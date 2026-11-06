#![cfg(feature = "js")]

use plf::{Context, Tera, context};

fn tera_with(code: &str) -> Tera {
    let mut tera = Tera::new_with_js(boa_engine::Context::default());
    tera.eval_js(code).expect("JS evaluation failed");
    tera
}

#[test]
fn eval_js_returns_value() {
    let mut tera = Tera::new_with_js(boa_engine::Context::default());
    let res = tera.eval_js("1 + 2").unwrap();
    assert_eq!(res.as_number(), Some(3.0));
}

#[test]
fn js_handler_receives_kwargs_object() {
    let tera = tera_with(
        r#"registerPlfHandler("greet", (kwargs) => `Hello ${kwargs.name}, ${kwargs.n * 2}`);"#,
    );
    let out = tera
        .render_str(
            "{{ greet(name=who, n=21) }}",
            &context! { who => "World" },
            false,
        )
        .unwrap();
    assert_eq!(out, "Hello World, 42");
}

#[test]
fn js_handler_converts_values_both_ways() {
    let tera = tera_with(
        r#"registerPlfHandler("roundtrip", (kwargs) => ({
            arr: kwargs.items.map((x) => x + 1),
            map: { key: kwargs.obj.a, nested: [true, null] },
            str: "ü€",
            big: 12345678901234567890n,
            neg: -5,
            float: 1.5,
        }));"#,
    );
    let out = tera
        .render_str(
            "{% set r = roundtrip(items=[1, 2, 3], obj={\"a\": \"b\"}) %}\
             {{ r.arr | join(sep=\",\") }}|{{ r.map.key }}|{{ r.map.nested[0] }}|{{ r.str }}|{{ r.big }}|{{ r.neg }}|{{ r.float }}",
            &Context::new(),
            false,
        )
        .unwrap();
    assert_eq!(out, "2,3,4|b|true|ü€|12345678901234567890|-5|1.5");
}

#[test]
fn js_handler_promise_is_awaited() {
    let tera = tera_with(
        r#"registerPlfHandler("later", async (kwargs) => { await null; return kwargs.v + "!"; });"#,
    );
    let out = tera
        .render_str("{{ later(v=\"done\") }}", &Context::new(), false)
        .unwrap();
    assert_eq!(out, "done!");
}

#[test]
fn js_handler_is_safe_flag() {
    let tera = tera_with(
        r#"registerPlfHandler("unsafe_html", () => "<b>");
           registerPlfHandler("safe_html", () => "<b>", true);"#,
    );
    let out = tera
        .render_str(
            "{{ unsafe_html() }}{{ safe_html() }}",
            &Context::new(),
            true,
        )
        .unwrap();
    assert_eq!(out, "&lt;b&gt;<b>");
}

#[test]
fn js_handler_errors_are_reported() {
    let tera = tera_with(r#"registerPlfHandler("boom", () => { throw new Error("nope"); });"#);
    let err = tera
        .render_str("{{ boom() }}", &Context::new(), false)
        .unwrap_err();
    let mut msg = format!("{err}");
    let mut source = std::error::Error::source(&err);
    while let Some(s) = source {
        msg.push_str(&format!(" | {s}"));
        source = s.source();
    }
    assert!(msg.contains("nope"), "{msg}");
}

#[test]
fn register_handler_rejects_bad_arguments() {
    let mut tera = Tera::new_with_js(boa_engine::Context::default());
    assert!(tera.eval_js(r#"registerPlfHandler(1, () => 1)"#).is_err());
    assert!(tera.eval_js(r#"registerPlfHandler("x", 1)"#).is_err());
}

#[test]
fn rust_and_js_functions_coexist() {
    let mut tera = tera_with(r#"registerPlfHandler("js_fn", () => "js");"#);
    tera.register_function("rust_fn", |_: plf::Kwargs, _: &plf::State| Ok("rust"));
    let out = tera
        .render_str("{{ rust_fn() }}-{{ js_fn() }}", &Context::new(), false)
        .unwrap();
    assert_eq!(out, "rust-js");
}

#[test]
fn register_handler_during_render_errors_instead_of_deadlocking() {
    let tera = tera_with(
        r#"registerPlfHandler("reg", () => { registerPlfHandler("other", () => 1); return "x"; });"#,
    );
    let err = tera
        .render_str("{{ reg() }}", &Context::new(), false)
        .unwrap_err();
    assert!(
        format!("{err}").contains("cannot be called while a template is rendering"),
        "{err}"
    );
}
