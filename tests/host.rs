// Author: David M. Anderson
// Built with AI assistance (Claude, Anthropic)

use std::io::Write;
use std::process::{Command, Stdio};

use lux::diagnostic::LuxError;
use lux::host::{self, CallResult, Instance, Limits, Program, Value};

const KEEP: &str = include_str!("fixtures/keep_host.lux");

const WALK: &str = "look\nhelp\ndance\nnorth\ntake gold\ntake key\ntake key\nopen door\neast\ntake torch\ndown\ntake gold\ntake gold\nup\nwest\nopen door\nopen door\nnorth\nsouth\nquit\n";

fn limits() -> Limits {
    Limits {
        steps: 1_000_000,
        depth: None,
        output_bytes: 1 << 20,
        value_bytes: 1 << 20,
    }
}

fn load(src: &str) -> Program {
    host::load(src).unwrap_or_else(|errs| panic!("should load, got: {}", errs[0].message))
}

fn load_err(src: &str) -> LuxError {
    host::load(src)
        .err()
        .unwrap_or_else(|| panic!("should be refused:\n{src}"))
        .remove(0)
}

fn start(src: &str, limits: Limits) -> Instance {
    host::instance(&load(src), limits).expect("a worker thread")
}

fn text(r: &CallResult) -> String {
    r.output.iter().map(|l| format!("{}\n", l.text())).collect()
}

fn err_of(r: CallResult) -> LuxError {
    match r.value {
        Err(e) => e,
        Ok(v) => panic!("should fail, returned {v}"),
    }
}

fn start_world() -> Value {
    Value::record(
        "World",
        [
            ("room", Value::case("Room", "entrance")),
            ("items", Value::Array(vec![])),
            ("doorOpen", Value::Bool(false)),
            ("playing", Value::Bool(true)),
        ],
    )
}

fn play_keep_through_host() -> String {
    let mut keep = start(KEEP, limits());
    let mut transcript = String::from("THE LITTLE KEEP\n\n");
    let first = keep.call("describeHere", vec![start_world()]);
    transcript += &text(&first);
    let mut world = first.value.expect("describeHere runs");
    transcript += "\n(type help for commands)\n";
    let mut lines = WALK.lines();
    while world.get_field("playing") == Some(&Value::Bool(true)) {
        transcript += "\n";
        let turn = match lines.next() {
            Some(line) => keep.call("step", vec![world.clone(), Value::string(line)]),
            None => keep.call("leave", vec![world.clone()]),
        };
        transcript += &text(&turn);
        world = turn
            .value
            .unwrap_or_else(|e| panic!("a turn failed: {}", e.message));
    }
    transcript
}

#[test]
fn the_keep_plays_the_same_through_the_host_as_under_lux_run() {
    let driver = "\nprint(\"THE LITTLE KEEP\")\nprint(\"\")\nvar world = World(room: Room.entrance, items: [], doorOpen: false, playing: true)\nworld = describeHere(world)\nprint(\"\")\nprint(\"(type help for commands)\")\nwhile world.playing {\n    print(\"\")\n    world = match readLine() {\n        some(let line) => step(world, line)\n        none           => leave(world)\n    }\n}\n";
    let path = std::env::temp_dir().join(format!("lux-host-keep-{}.lux", std::process::id()));
    std::fs::write(&path, format!("{KEEP}{driver}")).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_lux"))
        .arg("run")
        .arg(&path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(WALK.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let _ = std::fs::remove_file(&path);
    assert!(
        out.status.success(),
        "lux run should play the twin:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let expected = String::from_utf8_lossy(&out.stdout).into_owned();

    let transcript = play_keep_through_host();
    assert!(transcript.contains("You fill your pockets with coins"));
    assert_eq!(transcript, expected);
}

#[test]
fn the_same_calls_give_the_same_transcript_every_time() {
    assert_eq!(play_keep_through_host(), play_keep_through_host());
}

#[test]
fn the_outside_world_is_refused_at_load() {
    let cases = [
        (
            "readLine",
            "func f() -> string {\n    return match readLine() {\n        some(let l) => l\n        none => \"\"\n    }\n}\n",
        ),
        (
            "input",
            "func f() -> string {\n    return input(\"? \")\n}\n",
        ),
        ("args", "func f() -> [string] {\n    return args()\n}\n"),
        (
            "readFile",
            "func f() -> string {\n    return match readFile(\"x\") {\n        ok(let s) => s\n        err(let e) => e\n    }\n}\n",
        ),
        (
            "writeFile",
            "func f() {\n    match writeFile(\"x\", \"y\") {\n        ok(let _) => print(\"ok\")\n        err(let e) => print(e)\n    }\n}\n",
        ),
        (
            "run",
            "func f() -> int {\n    return match run(\"ls\", [\"-l\"]) {\n        ok(let o) => o.status\n        err(let _) => 1\n    }\n}\n",
        ),
    ];
    for (name, src) in cases {
        let err = load_err(src);
        assert!(
            err.message.contains(&format!("`{name}`")),
            "{name}: {}",
            err.message
        );
        assert!(err.note.is_some(), "{name} should carry a note");
        assert!(err.span.start > 0, "{name} should point at the call");
    }
}

#[test]
fn a_top_level_statement_and_a_main_are_refused_at_load() {
    let err = load_err("func f() -> int {\n    return 1\n}\nprint(f())\n");
    assert!(
        err.message.contains("top level") || err.message.contains("only declares"),
        "{}",
        err.message
    );
    assert_eq!(
        &"func f() -> int {\n    return 1\n}\nprint(f())\n"[err.span.start..err.span.start + 5],
        "print"
    );

    let err = load_err("func main() {\n    print(\"hi\")\n}\n");
    assert!(err.message.contains("`main`"), "{}", err.message);
}

#[test]
fn ordinary_mistakes_are_still_caught_at_load() {
    let err = load_err("func f() -> int {\n    return \"one\"\n}\n");
    assert!(err.message.contains("int"), "{}", err.message);
}

#[test]
fn a_call_that_runs_too_long_is_stopped_and_the_instance_stays_usable() {
    let src = "func spin() -> int {\n    while true {\n    }\n    return 0\n}\n\nfunc fine() -> int {\n    return 7\n}\n";
    let mut inst = start(
        src,
        Limits {
            steps: 5_000,
            ..limits()
        },
    );
    let spun = inst.call("spin", vec![]);
    assert!(spun.steps_used > 5_000);
    let err = err_of(spun);
    assert!(err.message.contains("too long"), "{}", err.message);
    assert_eq!(&src[err.span.start..err.span.start + 5], "while");
    assert_eq!(inst.call("fine", vec![]).value.unwrap(), Value::Int(7));
}

#[test]
fn runaway_recursion_is_a_lux_error_not_a_crash() {
    let src = "func down(n: int) -> int {\n    return down(n + 1)\n}\n";
    let mut inst = start(src, limits());
    let err = err_of(inst.call("down", vec![Value::Int(0)]));
    assert!(err.message.contains("nested calls"), "{}", err.message);
    assert!(
        err.note.as_deref().unwrap_or("").contains("host"),
        "{:?}",
        err.note
    );
}

#[test]
fn a_value_that_grows_past_the_cap_stops_the_call() {
    let doubling = "func grow() -> int {\n    var s = \"x\"\n    while true {\n        s = s + s\n    }\n    return 0\n}\n";
    let mut inst = start(doubling, limits());
    let err = err_of(inst.call("grow", vec![]));
    assert!(err.message.contains("too large"), "{}", err.message);

    let filling = "func fill() -> int {\n    var xs = [1]\n    while true {\n        xs += 1\n    }\n    return 0\n}\n";
    let mut inst = start(
        filling,
        Limits {
            value_bytes: 10_000,
            ..limits()
        },
    );
    let filled = inst.call("fill", vec![]);
    assert!(filled.steps_used < 5_000);
    let err = err_of(filled);
    assert!(err.message.contains("too large"), "{}", err.message);

    let nesting = "struct Nest {\n    kids: [Nest]\n}\n\nfunc nest() -> int {\n    var n = Nest(kids: [])\n    while true {\n        n = Nest(kids: [n, n])\n    }\n    return 0\n}\n";
    let mut inst = start(nesting, limits());
    let nested = inst.call("nest", vec![]);
    assert!(nested.steps_used < 200);
    let err = err_of(nested);
    assert!(err.message.contains("too large"), "{}", err.message);
}

#[test]
fn output_past_the_limit_is_dropped_and_flagged() {
    let src = "func chat() {\n    for i in 0..10000 {\n        print(\"line\", i)\n    }\n}\n";
    let mut inst = start(
        src,
        Limits {
            output_bytes: 1_000,
            ..limits()
        },
    );
    let r = inst.call("chat", vec![]);
    assert!(r.value.is_ok());
    assert!(r.output_truncated);
    assert!(r.output.len() < 200 && !r.output.is_empty());
    assert_eq!(r.output[0].text(), "line 0");
}

#[test]
fn eprint_lands_on_the_error_stream_in_order() {
    let src = "func both() {\n    print(\"one\")\n    eprint(\"two\")\n    print(\"three\")\n}\n";
    let mut inst = start(src, limits());
    let r = inst.call("both", vec![]);
    assert_eq!(
        r.output,
        vec![
            host::OutputLine::Out("one".into()),
            host::OutputLine::Err("two".into()),
            host::OutputLine::Out("three".into()),
        ]
    );
}

#[test]
fn a_wrong_value_from_the_host_is_a_clear_error_before_anything_runs() {
    let mut keep = start(KEEP, limits());
    let world = |fields: Vec<(&str, Value)>| Value::record("World", fields);

    let missing = world(vec![
        ("room", Value::case("Room", "hall")),
        ("items", Value::Array(vec![])),
        ("playing", Value::Bool(true)),
    ]);
    let err = err_of(keep.call("step", vec![missing, Value::string("look")]));
    assert!(
        err.message.contains("missing the field `doorOpen`"),
        "{}",
        err.message
    );

    let mut fields = start_world();
    if let Value::Struct { fields: f, .. } = &mut fields {
        f[0].1 = Value::case("Room", "attic");
    }
    let err = err_of(keep.call("step", vec![fields, Value::string("look")]));
    assert!(err.message.contains("no case `attic`"), "{}", err.message);
    assert!(
        err.note.as_deref().unwrap_or("").contains("entrance, hall"),
        "{:?}",
        err.note
    );

    let mut fields = start_world();
    if let Value::Struct { fields: f, .. } = &mut fields {
        f[1].1 = Value::Array(vec![Value::Int(3)]);
    }
    let err = err_of(keep.call("step", vec![fields, Value::string("look")]));
    assert!(
        err.message.contains("`w.items[0]` should be string"),
        "{}",
        err.message
    );

    let err = err_of(keep.call("step", vec![start_world(), Value::Range(0, 3)]));
    assert!(err.message.contains("range"), "{}", err.message);

    let err = err_of(keep.call("step", vec![start_world()]));
    assert!(err.message.contains("takes 2 values"), "{}", err.message);

    let err = err_of(keep.call("fly", vec![]));
    assert!(err.message.contains("no function `fly`"), "{}", err.message);

    let shuffled = world(vec![
        ("playing", Value::Bool(true)),
        ("doorOpen", Value::Bool(false)),
        ("items", Value::Array(vec![])),
        ("room", Value::case("Room", "entrance")),
    ]);
    let r = keep.call("step", vec![shuffled, Value::string("look")]);
    assert_eq!(
        r.output[0].text(),
        "You stand at the mouth of an old stone keep. Cold air drifts from a passage north."
    );
    assert_eq!(
        r.value
            .unwrap()
            .get_field("room")
            .and_then(Value::variant_name),
        Some("entrance")
    );
}

const SMALL: &str = "enum Room {\n    hall\n    cellar\n}\n\nstruct World {\n    room: Room\n    items: [string]\n    doorOpen: bool\n}\n\nfunc fresh() -> World {\n    return World(room: Room.hall, items: [], doorOpen: false)\n}\n";

#[test]
fn a_saved_value_fits_a_new_program_until_a_field_changes() {
    let old = load(SMALL);
    let mut inst = host::instance(&old, limits()).unwrap();
    let saved = inst.call("fresh", vec![]).value.unwrap();
    assert!(old.fits("World", &saved).is_ok());

    let new_case_and_func = load(
        &SMALL
            .replace("    cellar\n}", "    cellar\n    tower\n}")
            .replace(
                "func fresh",
                "func extra(w: World) -> bool {\n    return w.doorOpen\n}\n\nfunc fresh",
            ),
    );
    assert!(new_case_and_func.fits("World", &saved).is_ok());

    let reordered = load(
        &SMALL
            .replace(
                "    room: Room\n    items: [string]\n    doorOpen: bool\n",
                "    doorOpen: bool\n    room: Room\n    items: [string]\n",
            )
            .replace(
                "World(room: Room.hall, items: [], doorOpen: false)",
                "World(doorOpen: false, room: Room.hall, items: [])",
            ),
    );
    assert!(reordered.fits("World", &saved).is_ok());

    let new_field = load(
        &SMALL
            .replace(
                "    doorOpen: bool\n",
                "    doorOpen: bool\n    torchLit: bool\n",
            )
            .replace("doorOpen: false)", "doorOpen: false, torchLit: false)"),
    );
    let err = new_field.fits("World", &saved).unwrap_err();
    assert!(err.message.contains("torchLit"), "{}", err.message);

    let retyped = load(
        &SMALL
            .replace("    doorOpen: bool\n", "    doorOpen: int\n")
            .replace("doorOpen: false)", "doorOpen: 0)"),
    );
    let err = retyped.fits("World", &saved).unwrap_err();
    assert!(
        err.message.contains("`World.doorOpen` should be int"),
        "{}",
        err.message
    );

    let err = old.fits("Castle", &saved).unwrap_err();
    assert!(err.message.contains("`Castle`"), "{}", err.message);

    assert!(old.func("fresh").is_some());
    assert!(old.func("step").is_none());
    assert!(matches!(old.type_decl("Room"), Some(host::TypeDecl::Enum(cases)) if cases.len() == 2));
    assert!(
        matches!(old.type_decl("World"), Some(host::TypeDecl::Struct(fields)) if fields[1].name == "items")
    );
}

#[test]
fn instances_of_the_same_and_different_programs_do_not_interfere() {
    let counter = "func count(n: int) -> int {\n    print(\"count\", n)\n    return n + 1\n}\n";
    let greeter = "func greet(name: string) -> string {\n    print(\"hello\", name)\n    return \"hi \" + name\n}\n";
    let program = load(counter);
    let mut a = host::instance(&program, limits()).unwrap();
    let mut b = host::instance(&program, limits()).unwrap();
    let mut c = start(greeter, limits());
    let mut n = Value::Int(0);
    let mut m = Value::Int(100);
    for _ in 0..3 {
        n = a.call("count", vec![n]).value.unwrap();
        let g = c.call("greet", vec![Value::string("ada")]);
        assert_eq!(text(&g), "hello ada\n");
        assert_eq!(g.value.unwrap(), Value::string("hi ada"));
        m = b.call("count", vec![m]).value.unwrap();
    }
    assert_eq!(n, Value::Int(3));
    assert_eq!(m, Value::Int(103));
    assert_eq!(text(&a.call("count", vec![Value::Int(9)])), "count 9\n");
    let err = err_of(a.call("greet", vec![Value::string("x")]));
    assert!(err.message.contains("no function `greet`"));
}

#[test]
fn an_instance_can_cross_threads() {
    fn is_send<T: Send>() {}
    is_send::<Instance>();
    let mut inst = start("func one() -> int {\n    return 1\n}\n", limits());
    let handle = std::thread::spawn(move || inst.call("one", vec![]).value.unwrap());
    assert_eq!(handle.join().unwrap(), Value::Int(1));
}

#[test]
fn values_print_the_way_lux_prints_them() {
    let w = Value::record(
        "World",
        [
            ("items", Value::Array(vec![Value::string("key")])),
            ("open", Value::Bool(true)),
        ],
    );
    assert_eq!(w.to_string(), "World(items: [key], open: true)");
    assert_eq!(Value::some(Value::Float(2.0)).to_string(), "some(2.0)");
    assert_eq!(Value::none().to_string(), "none");
    assert_eq!(Value::case("Room", "hall").to_string(), "Room.hall");
}

const SPEAKER: &str = r#"
struct World { count: int }

func fresh() -> World {
    return World(count: 0)
}

func step(w: World, cmd: string) -> World {
    say("you said " + cmd, "hall")
    if cmd == "knock" {
        build(1, 2, 3, "stone")
    }
    print("noted")
    return World(count: w.count + 1)
}

func quiet(w: World) -> World {
    return w
}
"#;

const OFFERS: &[&str] = &[
    "func say(text: string, room: string)",
    "func build(x: int, y: int, z: int, kind: string)",
];

#[test]
fn calls_to_offered_functions_come_back_as_effects_in_order() {
    let program = host::load_with(SPEAKER, OFFERS).expect("the speaker loads");
    assert_eq!(program.offers(), ["say", "build"]);
    assert!(program.func("say").is_none());
    let mut world = host::instance(&program, limits()).expect("a worker thread");
    let w = world.call("fresh", vec![]).value.expect("fresh runs");

    let turn = world.call("step", vec![w.clone(), Value::string("knock")]);
    assert_eq!(text(&turn), "noted\n");
    assert_eq!(
        turn.effects,
        vec![
            host::Effect {
                name: "say".into(),
                args: vec![Value::string("you said knock"), Value::string("hall")],
            },
            host::Effect {
                name: "build".into(),
                args: vec![
                    Value::Int(1),
                    Value::Int(2),
                    Value::Int(3),
                    Value::string("stone")
                ],
            },
        ]
    );
    let again = world.call("step", vec![w.clone(), Value::string("hum")]);
    assert_eq!(again.effects.len(), 1, "effects start over with each call");
    assert!(world.call("quiet", vec![w]).effects.is_empty());
}

#[test]
fn an_offered_function_is_held_to_its_signature() {
    let wrong = SPEAKER.replace(r#"say("you said " + cmd, "hall")"#, r#"say(cmd)"#);
    let err = host::load_with(&wrong, OFFERS)
        .expect_err("too few arguments is refused")
        .remove(0);
    assert!(err.message.contains("`say`"), "{}", err.message);

    let typed = SPEAKER.replace(r#"build(1, 2, 3, "stone")"#, r#"build(1, 2, "3", "stone")"#);
    let err = host::load_with(&typed, OFFERS)
        .expect_err("a wrong type is refused")
        .remove(0);
    assert_eq!(err.message, "`build` expects `z` to be int, but got string");
}

#[test]
fn a_program_cannot_declare_an_offered_name_and_a_host_cannot_call_one() {
    let own = format!("{SPEAKER}\nfunc say(text: string, room: string) {{ print(text) }}\n");
    let err = host::load_with(&own, OFFERS)
        .err()
        .expect("refused")
        .remove(0);
    assert!(err.message.contains("the host offers"), "{}", err.message);
    assert!(
        err.span.start > 0,
        "the error points at the program's own declaration"
    );

    let program = host::load_with(SPEAKER, OFFERS).unwrap();
    let mut world = host::instance(&program, limits()).unwrap();
    let err = world
        .call("say", vec![Value::string("hi"), Value::string("hall")])
        .value
        .unwrap_err();
    assert!(
        err.message.contains("not one it can call"),
        "{}",
        err.message
    );
}

#[test]
fn an_offer_that_is_not_a_bare_signature_is_the_hosts_mistake() {
    for bad in [
        "func say(text: string) -> int",
        "say(text: string)",
        "func say(text: string) {} func more() {}",
    ] {
        let err = host::load_with(SPEAKER, &[bad]).expect_err(bad).remove(0);
        assert!(
            err.message.contains("the host's offer"),
            "{bad}: {}",
            err.message
        );
    }
}

#[test]
fn calling_out_past_the_output_budget_stops_the_call() {
    let chatty = r#"
func chatter(n: int) -> int {
    var i = 0
    while i < n {
        say("again", "hall")
        i = i + 1
    }
    return i
}
"#;
    let program = host::load_with(chatty, OFFERS).unwrap();
    let small = Limits {
        output_bytes: 256,
        ..limits()
    };
    let mut world = host::instance(&program, small).unwrap();
    let fine = world.call("chatter", vec![Value::Int(3)]);
    assert_eq!(fine.effects.len(), 3);
    let stopped = world.call("chatter", vec![Value::Int(1000)]);
    let err = stopped.value.unwrap_err();
    assert!(
        err.message.contains("more than it can take"),
        "{}",
        err.message
    );
    assert!(stopped.effects.is_empty());
}
