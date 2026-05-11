use crate::Value;
use crate::args::Kwargs;
use crate::errors::{Error, TeraResult};
use crate::value::FunctionResult;
use crate::vm::state::State;
use std::sync::Arc;

/// The function function type definition
pub trait Function<Res>: Sync + Send + 'static {
    /// The function type definition
    fn call(&self, kwargs: Kwargs, state: &State) -> Res;

    /// Whether the current function's output should be treated as safe, defaults to `false`
    /// Only needs to be defined if the filter returns a string
    fn is_safe(&self) -> bool {
        false
    }
}

impl<Func, Res> Function<Res> for Func
where
    Func: Fn(Kwargs, &State) -> Res + Sync + Send + 'static,
    Res: FunctionResult,
{
    fn call(&self, kwargs: Kwargs, state: &State) -> Res {
        (self)(kwargs, state)
    }
}

type FunctionFunc = dyn Fn(Kwargs, &State) -> TeraResult<Value> + Sync + Send + 'static;

#[derive(Clone)]
pub(crate) enum FunctionInner {
    Rust(Arc<FunctionFunc>),
    #[cfg(feature = "js")]
    Js(boa_engine::object::builtins::JsFunction),
}

#[derive(Clone)]
pub(crate) struct StoredFunction {
    inner: FunctionInner,
    is_safe: bool,
}

impl StoredFunction {
    pub fn new<Func, Res>(f: Func) -> Self
    where
        Func: Function<Res>,
        Res: FunctionResult,
    {
        let is_safe = f.is_safe();
        let closure = move |kwargs, state: &State| -> TeraResult<Value> {
            f.call(kwargs, state).into_result()
        };

        StoredFunction {
            inner: FunctionInner::Rust(Arc::new(closure)),
            is_safe,
        }
    }

    #[cfg(feature = "js")]
    pub fn new_js(js_func: boa_engine::object::builtins::JsFunction, is_safe: bool) -> Self {
        StoredFunction {
            inner: FunctionInner::Js(js_func),
            is_safe,
        }
    }

    pub fn call(&self, kwargs: Kwargs, state: &mut State) -> TeraResult<Value> {
        match &self.inner {
            FunctionInner::Rust(func) => func(kwargs, state),
            #[cfg(feature = "js")]
            FunctionInner::Js(js_func) => {
                let Some(ref mut js_context) = state.js_context else {
                    return Err(Error::message(
                        "JS function called but no JS context is available in the state",
                    ));
                };
                use boa_engine::{
                    JsValue,
                    value::{TryFromJs, TryIntoJs},
                };

                match js_func.call(
                    &JsValue::undefined(),
                    &[kwargs.try_into_js(*js_context).map_err(|e| {
                        Error::message(format!("Error converting kwargs to JS value: {e}"))
                    })?],
                    *js_context,
                ) {
                    Ok(result) => {
                        let val = if let Some(promise) = result.as_promise() {
                            use boa_engine::object::builtins::JsPromise;

                            let empty_promise = JsPromise::new(
                                |resolvers, ctx| {
                                    use boa_engine::job::{Job, NativeJob, TimeoutJob};

                                    let rejector = resolvers.reject.clone();
                                    let job = TimeoutJob::new(
                                        NativeJob::new(move |ctx| {
                                            use boa_engine::js_string;

                                            rejector.call(
                                                &JsValue::undefined(),
                                                &[JsValue::from(js_string!(
                                                    "Promise timed out after 2 seconds"
                                                ))],
                                                ctx,
                                            )?;
                                            Ok(JsValue::undefined())
                                        }),
                                        2000,
                                    );
                                    ctx.enqueue_job(Job::TimeoutJob(job));
                                    Ok(JsValue::undefined())
                                },
                                *js_context,
                            );

                            let result =
                                JsPromise::race([promise.clone(), empty_promise], *js_context);

                            result.await_blocking(*js_context).map_err(|e| {
                                Error::message(format!("Error awaiting JS promise: {e}"))
                            })?
                        } else {
                            result
                        };
                        Value::try_from_js(&val, *js_context).map_err(|e| {
                            Error::message(format!("Error converting JS result to Tera value: {e}"))
                        })
                    }
                    Err(e) => Err(Error::message(format!("Error calling JS function: {e}"))),
                }
            }
        }
    }

    pub fn is_safe(&self) -> bool {
        self.is_safe
    }
}

/// Upper bound on the number of elements `range()` will produce to avoid OOM.
const MAX_RANGE_LEN: usize = 100_000;

pub(crate) fn range(kwargs: Kwargs, _: &State) -> TeraResult<Vec<isize>> {
    let start = kwargs.get::<i128>("start")?.unwrap_or_default();
    let end = kwargs.must_get::<i128>("end")?;
    let step_by = kwargs.get::<i128>("step_by")?.unwrap_or(1);
    if start > end && step_by > 0 {
        return Err(Error::message(
            "Function `range` was called with a `start` argument greater than the `end` one",
        ));
    }
    if step_by == 0 {
        return Err(Error::message(
            "Function `range` was called with a `step_by` argument of 0",
        ));
    }

    let len = if step_by > 0 {
        let span = end - start;
        (span + step_by - 1) / step_by
    } else if start <= end {
        0
    } else {
        let span = start - end;
        let step = -step_by;
        (span + step - 1) / step
    };
    if len > MAX_RANGE_LEN as i128 {
        return Err(Error::message(format!(
            "Function `range` would produce {len} elements, which exceeds the limit of {MAX_RANGE_LEN}"
        )));
    }

    let mut values = Vec::with_capacity(len as usize);
    for i in 0..len {
        values.push((start + i * step_by) as isize);
    }
    Ok(values)
}

pub(crate) fn throw(kwargs: Kwargs, _: &State) -> TeraResult<bool> {
    let message = kwargs.must_get::<&str>("message")?;
    Err(Error::message(message))
}
