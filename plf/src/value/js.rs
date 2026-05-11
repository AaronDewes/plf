use std::collections::HashSet;
use std::sync::Arc;

use boa_engine::object::builtins::{JsArray, JsUint8Array};
use boa_engine::property::{PropertyDescriptor, PropertyKey};
use boa_engine::value::{TryFromJs, TryIntoJs};
use boa_engine::{js_string, prelude::*};
use num_bigint::Sign;
use num_traits::cast::ToPrimitive;

use crate::Value;
use crate::value::{Key, SmartString, ValueInner};

impl Into<PropertyKey> for &Key<'_> {
    fn into(self) -> PropertyKey {
        match self {
            Key::String(s) => PropertyKey::String(js_string!(&**s)),
            Key::U64(u) => PropertyKey::String(js_string!(u.to_string())),
            Key::I64(i) => PropertyKey::String(js_string!(i.to_string())),
            Key::Bool(b) => PropertyKey::String(js_string!(b.to_string())),
            Key::U128(u) => PropertyKey::String(js_string!(u.to_string())),
            Key::I128(i) => PropertyKey::String(js_string!(i.to_string())),
            Key::Str(s) => PropertyKey::String(js_string!(*s)),
        }
    }
}

impl TryIntoJs for Value {
    fn try_into_js(&self, context: &mut Context) -> boa_engine::JsResult<JsValue> {
        match &self.inner {
            ValueInner::Undefined => Ok(JsValue::undefined()),
            // TODO: Is this correct?
            ValueInner::None => Ok(JsValue::null()),
            ValueInner::Bool(b) => Ok(JsValue::from(*b)),
            ValueInner::U64(u) => Ok(JsValue::from(*u)),
            ValueInner::I64(i) => Ok(JsValue::from(*i)),
            ValueInner::F64(f) => Ok(JsValue::from(*f)),
            ValueInner::U128(u) => (boa_engine::bigint::JsBigInt::from(**u)).try_into_js(context),
            ValueInner::I128(i) => (boa_engine::bigint::JsBigInt::from(**i)).try_into_js(context),
            ValueInner::String(smart_string) => {
                Ok(JsValue::from(js_string!(smart_string.as_str())))
            }
            ValueInner::Array(values) => {
                let arr = JsArray::new(context);
                for val in values.iter() {
                    let val = val.try_into_js(context)?;
                    arr.push(val, context)?;
                }
                Ok(arr.into())
            }
            ValueInner::Map(hash_map) => {
                let js_obj = JsObject::with_object_proto(context.intrinsics());
                for (key, value) in hash_map.iter() {
                    let property = PropertyDescriptor::builder()
                        .value(value.try_into_js(context)?)
                        .writable(true)
                        .enumerable(true)
                        .configurable(true);
                    js_obj.insert_property(key, property);
                }

                Ok(js_obj.into())
            }
            ValueInner::Bytes(items) => {
                let arr = JsUint8Array::from_iter(items.iter().copied(), context)?;
                Ok(arr.into())
            }
        }
    }
}

fn value_from_js_inner(
    value: &JsValue,
    context: &mut Context,
    seen_objects: &mut HashSet<JsObject>,
) -> boa_engine::JsResult<ValueInner> {
    let inner = match value.variant() {
        JsVariant::Null => ValueInner::None,
        JsVariant::Undefined => ValueInner::Undefined,
        JsVariant::Boolean(b) => ValueInner::Bool(b),
        JsVariant::String(js_string) => ValueInner::String(SmartString::new(
            &js_string.as_str().to_std_string_escaped(),
            super::StringKind::Normal,
        )),
        JsVariant::Float64(f) => ValueInner::F64(f),
        JsVariant::Integer32(i) => ValueInner::I64(i as i64),
        JsVariant::BigInt(js_big_int) => {
            let inner = js_big_int.as_inner();
            if inner.sign() == Sign::Plus {
                ValueInner::U128(Box::new(inner.to_u128().ok_or(
                    JsNativeError::range().with_message("BigInt value is too large to fit in u128"),
                )?))
            } else {
                ValueInner::I128(Box::new(inner.to_i128().ok_or(
                    JsNativeError::range().with_message("BigInt value is too small to fit in i128"),
                )?))
            }
        }
        JsVariant::Object(obj) => {
            if seen_objects.contains(&obj) {
                return Err(JsNativeError::typ()
                    .with_message("cyclic object value")
                    .into());
            }
            seen_objects.insert(obj.clone());
            let mut value_by_prop_key = |property_key, context: &mut Context| {
                obj.borrow().properties().get(&property_key).and_then(|x| {
                    x.value()
                        .map(|val| value_from_js_inner(val, context, seen_objects))
                })
            };

            if obj.is_array() {
                let arr = JsArray::from_object(obj.clone())?;
                let len = arr.length(context)?;
                let mut arr = Vec::with_capacity(len as usize);

                for k in 0..len as u32 {
                    let val = value_by_prop_key(k.into(), context);
                    match val {
                        Some(val) => arr.push(Value { inner: val? }),

                        // Undefined in array. Substitute with null as Value doesn't support Undefined.
                        None => arr.push(Value {
                            inner: ValueInner::None,
                        }),
                    }
                }
                // Passing the object rather than its clone that was inserted to the set should be fine
                // as they hash to the same value and therefore HashSet can still remove the clone
                seen_objects.remove(&obj);
                ValueInner::Array(Arc::new(arr))
            } else {
                let mut map = crate::HashMap::new();

                let keys = obj.own_property_keys(context)?;

                for key in keys {
                    let tera_key = match &key {
                        PropertyKey::String(s) => {
                            Key::String(Arc::<str>::from(s.as_str().to_std_string_escaped()))
                        }
                        PropertyKey::Index(i) => Key::U64(u64::from(i.get())),
                        PropertyKey::Symbol(_) => {
                            continue;
                        }
                    };
                    let val = value_by_prop_key(key, context);
                    if let Some(val) = val {
                        map.insert(tera_key, Value { inner: val? });
                    }
                }

                ValueInner::Map(Arc::new(map))
            }
        }
        JsVariant::Symbol(_) => {
            return Err(JsNativeError::typ()
                .with_message("Cannot convert JS Symbol to Tera Value")
                .into());
        }
    };
    Ok(inner)
}

impl TryFromJs for Value {
    fn try_from_js(value: &JsValue, context: &mut Context) -> boa_engine::JsResult<Self> {
        let inner = value_from_js_inner(value, context, &mut HashSet::new())?;
        Ok(Value { inner })
    }
}
