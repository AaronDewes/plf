use boa_engine::{Context, JsValue, property::PropertyDescriptor, value::TryIntoJs};

use crate::Kwargs;

impl TryIntoJs for Kwargs {
    fn try_into_js(&self, context: &mut Context) -> boa_engine::JsResult<JsValue> {
        let js_obj = boa_engine::object::JsObject::with_object_proto(context.intrinsics());
        for (key, value) in self.values.iter() {
            let property = PropertyDescriptor::builder()
                .value(value.try_into_js(context)?)
                .writable(true)
                .enumerable(true)
                .configurable(true);
            js_obj.insert_property(key, property);
        }

        Ok(js_obj.into())
    }
}
