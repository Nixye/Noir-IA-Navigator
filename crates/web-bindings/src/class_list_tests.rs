//! Recovery R1 tests: fresh work on the restored V13, not recovered V14–V49.
use super::*;
fn session() -> DocumentSession {
    let mut s = DocumentSession::new(
        web_html::parse("<div id=x class=' a  b a '></div><div id=y></div>").unwrap(),
    );
    s.execute("const x=document.getElementById('x');const y=document.getElementById('y');const l=x.classList;").unwrap();
    s
}
fn yes(code: &str) {
    assert_eq!(
        session().execute(code).unwrap(),
        Value::Bool(true),
        "{code}"
    );
}
#[test]
fn class_list_live_order_identity_and_canonical_indices() {
    yes(
        "l===x.classList && l.length===2 && l[0]==='a' && l[1]==='b' && l[2]===undefined && l['01']===undefined && l['-0']===undefined && l['4294967295']===undefined && (0 in l) && !(2 in l) && !('01' in l);",
    );
    yes(
        "x.setAttribute('class','c c d');l.length===2 && l[0]==='c' && l[1]==='d' && l===x.classList;",
    );
}
#[test]
fn class_list_raw_value_and_stringifier() {
    yes("l.value===' a  b a ' && l.toString()===' a  b a ' && String(l)===' a  b a ';");
    yes(
        "l.value=' z  z q ';x.getAttribute('class')===' z  z q ' && l.value===' z  z q ' && l.length===2;",
    );
}
#[test]
fn class_list_variadic_add_remove_are_ordered_and_deduplicate() {
    yes("l.add('b','c','c','d');l.value==='a b c d' && l.length===4;");
    yes("l.remove('a','b','a');l.value==='' && l.length===0;");
}
#[test]
fn class_list_zero_arguments_normalize_but_do_not_create_absent_attribute() {
    yes(
        "l.add();l.value==='a b' && y.classList.add()===undefined && y.getAttribute('class')===null;",
    );
    yes(
        "l.remove();l.value==='a b' && y.classList.remove()===undefined && y.getAttribute('class')===null;",
    );
    yes("l.add('a');l.value==='a b';");
    yes("l.remove('missing');l.value==='a b';");
}
#[test]
fn class_list_toggle_noop_preserves_raw_and_undefined_is_omitted() {
    yes(
        "l.toggle('a',true)===true && l.value===' a  b a ' && l.toggle('c',false)===false && l.value===' a  b a ';",
    );
    yes(
        "l.toggle('a',undefined)===false && l.value==='b' && l.toggle('c',undefined)===true && l.value==='b c';",
    );
    yes(
        "l.toggle('a',{})===true && l.value===' a  b a ' && l.toggle('a',null)===false && l.value==='b';",
    );
}
#[test]
fn class_list_replace_preserves_first_position_and_updates_equal_token() {
    yes("l.value='a b c';l.replace('a','c')===true && l.value==='c b';");
    yes("l.value='c b a';l.replace('a','c')===true && l.value==='c b';");
    yes("l.replace('missing','c')===false && l.value===' a  b a ';");
    yes("l.replace('a','a')===true && l.value==='a b';");
    yes("l.replace('a','c')===true && l.value==='c b';");
}
#[test]
fn class_list_contains_does_not_validate_query() {
    yes("!l.contains('') && !l.contains('a b') && !l.contains('a\\tb') && l.contains('a');");
}
#[test]
fn class_list_ascii_whitespace_excludes_vertical_tab_and_nonbreaking_space() {
    yes("l.value='a\\tb\\nc\\rd\\fe';l.length===5 && l[4]==='e';");
    yes(
        "l.value='';l.add('a\\vb','c\\u00a0d');l.length===2 && l.contains('a\\vb') && l.contains('c\\u00a0d');",
    );
}
#[test]
fn class_list_invalid_variadic_token_does_not_partially_mutate() {
    yes(
        "let failed=false;try{l.add('c','bad token','d');}catch(e){failed=true;}failed && l.value===' a  b a ';",
    );
    yes(
        "let failed=false;try{l.remove('a','');}catch(e){failed=true;}failed && l.value===' a  b a ';",
    );
    yes(
        "let failed=false;try{l.replace('a','');}catch(e){failed=true;}failed && l.value===' a  b a ';",
    );
}
#[test]
fn class_list_all_conversions_precede_validation_and_use_post_hook_state() {
    yes(
        "let order='';let failed=false;try{l.add({toString:function(){order+='1';return 'bad token';}},{toString:function(){order+='2';x.setAttribute('class','hook');return 'c';}});}catch(e){failed=true;}failed && order==='12' && l.value==='hook';",
    );
    yes(
        "let n=0;l.add({toString:function(){n++;x.setAttribute('class','hook');return 'new';}});n===1 && l.value==='hook new';",
    );
    yes(
        "l.value={toString:function(){x.setAttribute('class','hook');return ' raw raw ';}};l.value===' raw raw ' && l.length===1;",
    );
}
#[test]
fn class_list_throwing_conversion_stops_later_hooks_and_own_mutation() {
    yes(
        "let order='';let failed=false;try{l.add({toString:function(){order+='1';x.setAttribute('class','hook');throw 'stop';}},{toString:function(){order+='2';return 'new';}});}catch(e){failed=true;}failed && order==='1' && l.value==='hook';",
    );
}
#[test]
fn class_list_receiver_and_readonly_writes_reject_before_hooks() {
    yes(
        "let n=0;let failed=false;try{l.add.call(x,{toString:function(){n++;return 'c';}});}catch(e){failed=true;}failed && n===0 && l.value===' a  b a ';",
    );
    yes(
        "let n=0;let failed=false;try{l.length={toString:function(){n++;return '0';}};}catch(e){failed=true;}failed && n===0 && l.length===2;",
    );
    yes("let failed=false;try{l[0]='z';}catch(e){failed=true;}failed && l[0]==='a';");
}
#[test]
fn class_list_detached_method_with_valid_receiver_and_detached_element() {
    yes("const add=l.add;x.remove();add.call(l,'c','d');l.value==='a b c d' && l===x.classList;");
}
#[test]
fn class_list_required_arguments_and_primitive_conversion() {
    yes(
        "let failures=0;try{l.contains();}catch(e){failures++;}try{l.toggle();}catch(e){failures++;}try{l.replace('a');}catch(e){failures++;}try{l.item();}catch(e){failures++;}failures===4 && l.value===' a  b a ';",
    );
    yes("l.value='';l.add(undefined,null,true,7);l.value==='undefined null true 7';");
}
#[test]
fn class_list_item_unsigned_conversion_and_live_hook() {
    yes(
        "l.item(undefined)==='a' && l.item(NaN)==='a' && l.item(Infinity)==='a' && l.item(-1)===null && l.item(4294967296)==='a' && l.item(1.9)==='b' && l.item(2)===null;",
    );
    yes("let n=0;l.item({valueOf:function(){n++;l.value='x y';return 1;}})==='y' && n===1;");
}
#[test]
fn class_list_mutation_and_read_budgets_reject_without_partial_attribute_commit() {
    for budget in ["mutation", "string", "read"] {
        let mut document = web_html::parse("<p class='a'></p>").unwrap();
        let id = document.elements().find(|(_, e)| e.tag == "p").unwrap().0;
        let mut state = State::default();
        match budget {
            "mutation" => state.mutations = 2048,
            "string" => state.string_bytes = 256 * 1024,
            _ => state.read_work = 2_000_000,
        }
        let mut host = DomHost {
            document: &mut document,
            state: &mut state,
        };
        let error = host
            .class_call_primitive(CLASS + id as u64, "add", &[Value::string("b")])
            .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Resource);
        assert_eq!(document.element(id).unwrap().attr("class"), Some("a"));
    }
}
#[test]
fn class_list_large_result_and_many_tokens_are_bounded_before_dom_commit() {
    for raw in [
        "a".repeat(32_768),
        (0..4096)
            .map(|i| format!("t{i}"))
            .collect::<Vec<_>>()
            .join(" "),
    ] {
        let mut document = web_html::parse(&format!("<p class='{raw}'></p>")).unwrap();
        let id = document.elements().find(|(_, e)| e.tag == "p").unwrap().0;
        let mut state = State::default();
        let mut host = DomHost {
            document: &mut document,
            state: &mut state,
        };
        assert_eq!(
            host.class_call_primitive(CLASS + id as u64, "add", &[Value::string("new")])
                .unwrap_err()
                .kind,
            ErrorKind::Resource
        );
        assert_eq!(
            document.element(id).unwrap().attr("class"),
            Some(raw.as_str())
        );
    }
}
#[test]
fn class_list_argument_cap_precedes_conversion_and_value_size_is_bounded() {
    let mut document = web_html::parse("<p class='a'></p>").unwrap();
    let id = document.elements().find(|(_, e)| e.tag == "p").unwrap().0;
    let mut state = State::default();
    let mut host = DomHost {
        document: &mut document,
        state: &mut state,
    };
    assert_eq!(
        host.class_call_primitive(CLASS + id as u64, "add", &vec![Value::string("a"); 257])
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(
        host.class_set_value(CLASS + id as u64, "b".repeat(32_769))
            .unwrap_err()
            .kind,
        ErrorKind::Resource
    );
    assert_eq!(document.element(id).unwrap().attr("class"), Some("a"));
}
#[test]
fn class_list_opt_in_stringifier_supports_all_primitive_hints_and_domstring() {
    yes("l.value='17';Number(l)===17 && +l===17 && l+2==='172' && String(l)==='17';");
    yes("y.classList.value=l;y.getAttribute('class')===' a  b a ' && y.classList.length===2;");
    yes("l.value='new';y.classList.add(l);y.classList.contains('new');");
    yes(
        "let failures=0;try{String(x);}catch(e){failures++;}try{String(document);}catch(e){failures++;}try{String(x.style);}catch(e){failures++;}failures===3;",
    );
}
#[test]
fn class_list_stringifier_uses_live_attribute_and_read_budget() {
    yes("String(l)===' a  b a ' && (l.value='after',String(l)==='after');");
    let mut document = web_html::parse("<p class='a'></p>").unwrap();
    let id = document.elements().find(|(_, e)| e.tag == "p").unwrap().0;
    let mut state = State {
        read_work: 2_000_000,
        ..State::default()
    };
    let mut host = DomHost {
        document: &mut document,
        state: &mut state,
    };
    assert_eq!(
        host.stringifier(CLASS + id as u64).unwrap_err().kind,
        ErrorKind::Resource
    );
    assert_eq!(host.stringifier(NODE + id as u64).unwrap(), None);
}
