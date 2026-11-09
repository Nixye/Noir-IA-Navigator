use super::*;

fn session() -> DocumentSession {
    DocumentSession::new(web_html::parse("<p id=out>safe</p>").unwrap())
}
fn output(session: &DocumentSession) -> String {
    session
        .document
        .text_content(session.query_selector("#out").unwrap().unwrap())
}

#[test]
fn aliases_arity_this_order_timestamp_and_microtasks() {
    let mut s = session();
    assert_eq!(s.execute("requestAnimationFrame===window.requestAnimationFrame && self.requestAnimationFrame===globalThis.requestAnimationFrame && cancelAnimationFrame===window.cancelAnimationFrame && requestAnimationFrame.length===1 && cancelAnimationFrame.length===1 && 'requestAnimationFrame' in window && typeof requestAnimationFrame==='function';").unwrap(), Value::Bool(true));
    s.execute("var seen='';requestAnimationFrame(function(t){seen+='a'+t+':'+(this===window)+':'+arguments.length;Promise.resolve().then(()=>{seen+='m';});});requestAnimationFrame(t=>{seen+='b'+t;});").unwrap();
    s.run_due_timers(999).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::string(""));
    s.run_animation_frame(12.5).unwrap();
    assert_eq!(
        s.execute("seen;").unwrap(),
        Value::string("a12.5:true:1mb12.5")
    );
    assert!(!s.animation_frame_pending());
    assert!(s.state.events.is_empty());
}

#[test]
fn cancellation_before_and_during_snapshot_and_new_registration() {
    let mut s = session();
    s.execute("var seen='';var canceled=requestAnimationFrame(()=>{seen+='x';});cancelAnimationFrame(canceled);requestAnimationFrame(()=>{seen+='a';cancelAnimationFrame(later);requestAnimationFrame(()=>{seen+='c';});});var later=requestAnimationFrame(()=>{seen+='b';});").unwrap();
    s.run_animation_frame(1.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::string("a"));
    assert!(s.animation_frame_pending());
    s.run_animation_frame(1.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::string("ac"));
}

#[test]
fn full_snapshot_is_not_extended_by_reusing_canceled_slots() {
    let mut s = session();
    s.execute("var seen=0;var last;requestAnimationFrame(()=>{seen++;cancelAnimationFrame(last);requestAnimationFrame(()=>{seen+=100;});});for(var i=0;i<31;i++){last=requestAnimationFrame(()=>{seen++;});}").unwrap();
    assert_eq!(s.state.animation.pending.len(), 32);
    s.run_animation_frame(1.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::Number(31.));
    s.run_animation_frame(2.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::Number(131.));
}

#[test]
fn ordinary_error_reports_and_continues_batch() {
    let mut s = session();
    s.execute("var seen='';onerror=()=>{seen+='e';};requestAnimationFrame(()=>{seen+='a';throw 'ordinary';});requestAnimationFrame(()=>{seen+='b';});").unwrap();
    assert_ne!(
        s.run_animation_frame(3.).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(s.execute("seen;").unwrap(), Value::string("aeb"));
}

#[test]
fn fatal_frame_rolls_back_only_failed_callback_and_clears_every_task() {
    let mut s = session();
    s.execute("requestAnimationFrame(()=>{document.getElementById('out').textContent='first';});requestAnimationFrame(()=>{document.getElementById('out').textContent='bad';requestAnimationFrame(()=>{});try{while(true){}}catch(e){document.getElementById('out').textContent='caught';}});requestAnimationFrame(()=>{document.getElementById('out').textContent='later';});setTimeout(()=>{},1);").unwrap();
    assert_eq!(
        s.run_animation_frame(20.).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(output(&s), "first");
    assert_eq!(s.state.animation.next_handle.load(Ordering::Relaxed), 4);
    assert!(!s.animation_frame_pending());
    assert!(s.state.animation.pending.is_empty());
    assert_eq!(s.next_timer_ms(), None);
    assert!(s.scripts_disabled);
    assert!(s.run_animation_frame(19.).is_err());
    s.run_animation_frame(20.).unwrap();
    assert_eq!(output(&s), "first");
}

#[test]
fn all_resource_entrypoints_clear_frames_without_refunding_identity() {
    for mode in ["execute", "timer", "event", "onerror"] {
        let mut s = session();
        s.execute("requestAnimationFrame(()=>{});").unwrap();
        let error = match mode {
            "execute" => s
                .execute("requestAnimationFrame(()=>{});while(true){}")
                .unwrap_err(),
            "timer" => {
                s.execute("setTimeout(()=>{requestAnimationFrame(()=>{});while(true){}},0);")
                    .unwrap();
                s.run_due_timers(0).unwrap_err()
            }
            "event" => {
                s.execute("document.getElementById('out').onclick=()=>{requestAnimationFrame(()=>{});while(true){}};").unwrap();
                let node = s.query_selector("#out").unwrap().unwrap();
                s.dispatch_event(node, "click").unwrap_err()
            }
            _ => {
                s.execute("onerror=()=>{requestAnimationFrame(()=>{});while(true){}};requestAnimationFrame(()=>{throw 'ordinary';});").unwrap();
                s.run_animation_frame(1.).unwrap_err()
            }
        };
        assert_eq!(error.kind, ErrorKind::Resource, "{mode}");
        assert!(s.scripts_disabled, "{mode}");
        assert!(s.state.animation.pending.is_empty(), "{mode}");
        assert!(
            s.state.animation.next_handle.load(Ordering::Relaxed) >= 2,
            "{mode}"
        );
    }
}

#[test]
fn pending_and_snapshot_closures_survive_collection() {
    let mut s = session();
    s.execute("var result='';function schedule(v){requestAnimationFrame(()=>{result+=v;});}schedule('a');schedule('b');").unwrap();
    s.runtime
        .collect_garbage(
            &[],
            &DomHost {
                document: &mut s.document,
                state: &mut s.state,
            },
        )
        .unwrap();
    s.run_animation_frame(2.).unwrap();
    assert_eq!(s.execute("result;").unwrap(), Value::string("ab"));
}

#[test]
fn queue_and_handle_limits_are_fatal_and_handles_never_reused() {
    let mut s = session();
    assert_eq!(s.execute("var first=requestAnimationFrame(()=>{});cancelAnimationFrame(first);requestAnimationFrame(()=>{});").unwrap(), Value::Number(2.));
    s.state
        .animation
        .next_handle
        .store(MAX_HANDLE - 1, Ordering::Relaxed);
    assert_eq!(
        s.execute("requestAnimationFrame(()=>{});").unwrap(),
        Value::Number(MAX_HANDLE as f64)
    );
    assert_eq!(
        s.execute("requestAnimationFrame(()=>{});")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(s.state.animation.pending.is_empty());
    let mut s = session();
    s.execute("for(var i=0;i<32;i++){requestAnimationFrame(()=>{});}")
        .unwrap();
    assert_eq!(
        s.execute("try{requestAnimationFrame(()=>{});}catch(e){}")
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert!(s.state.animation.pending.is_empty());
}

#[test]
fn invalid_timestamps_are_atomic_and_equal_is_accepted() {
    let mut s = session();
    s.execute("var seen=0;requestAnimationFrame(()=>{seen++;});")
        .unwrap();
    for timestamp in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1.,
        MAX_ANIMATION_FRAME_TIMESTAMP_MS + 1.,
    ] {
        assert!(s.run_animation_frame(timestamp).is_err());
        assert_eq!(s.last_animation_frame_timestamp_ms, None);
        assert!(s.animation_frame_pending());
    }
    s.run_animation_frame(4.).unwrap();
    s.execute("requestAnimationFrame(()=>{seen++;});").unwrap();
    assert!(s.run_animation_frame(3.).is_err());
    assert!(s.animation_frame_pending());
    s.run_animation_frame(4.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::Number(2.));
}

#[test]
fn argument_validation_real_coercion_and_safe_integer_cancellation() {
    let mut s = session();
    for source in [
        "requestAnimationFrame();",
        "requestAnimationFrame('code');",
        "requestAnimationFrame({});",
        "cancelAnimationFrame();",
        "cancelAnimationFrame(1n);",
        "requestAnimationFrame.call({},()=>{});",
    ] {
        assert_ne!(
            s.execute(source).unwrap_err().kind,
            ErrorKind::Resource,
            "{source}"
        );
    }
    s.execute("var seen=0;var id=requestAnimationFrame(()=>{seen++;});cancelAnimationFrame(id+0.5);cancelAnimationFrame(-id);cancelAnimationFrame(NaN);cancelAnimationFrame(Infinity);").unwrap();
    assert!(s.animation_frame_pending());
    assert_eq!(s.execute("var converted=0;cancelAnimationFrame({valueOf(){converted++;return String(id);}});converted;").unwrap(), Value::Number(1.));
    assert!(!s.animation_frame_pending());
    s.run_animation_frame(0.).unwrap();
    assert_eq!(s.execute("seen;").unwrap(), Value::Number(0.));
}
