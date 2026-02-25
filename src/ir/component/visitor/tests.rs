use crate::ir::component::visitor::driver::VisitEvent;
use crate::ir::component::visitor::events_structural::get_structural_events;
use crate::ir::component::visitor::events_topological::get_topological_events;
use crate::ir::component::visitor::VisitCtx;
use crate::Component;
use serde_json::Value;
use std::fs;
use std::mem::discriminant;
use std::path::Path;
use std::process::Command;

const WASM_TOOLS_TEST_COMP_INPUTS: &str = "./tests/wasm-tools/component-model";

fn get_events<'ir>(
    comp: &'ir Component<'ir>,
    get_evts: fn(&'ir crate::Component<'ir>, &mut VisitCtx<'ir>, &mut Vec<VisitEvent<'ir>>),
) -> Vec<VisitEvent<'ir>> {
    let mut ctx = VisitCtx::new(comp);
    let mut events = Vec::new();
    get_evts(comp, &mut ctx, &mut events);

    events
}

fn events_are_equal(evts0: &Vec<VisitEvent>, evts1: &Vec<VisitEvent>) {
    for (a, b) in evts0.iter().zip(evts1.iter()) {
        assert_eq!(discriminant(a), discriminant(b));
    }
}

fn test_event_generation(filename: &str) {
    println!("\nfilename: {:?}", filename);
    let buff = wat::parse_file(filename).expect("couldn't convert the input wat to Wasm");
    let original = wasmprinter::print_bytes(&buff).expect("couldn't convert original Wasm to wat");
    println!("original: {:?}", original);

    let comp = Component::parse(&buff, false, false).expect("Unable to parse");
    let evts_struct = get_events(&comp, get_structural_events);
    let evts_topo = get_events(&comp, get_topological_events);
    events_are_equal(&evts_struct, &evts_topo);
}

#[test]
fn test_equivalent_visit_events_wast_components() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_async() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/async");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_error_context() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/error-context");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_gc() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/gc");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_shared() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/shared-everything-threads");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

#[test]
fn test_equivalent_visit_events_wast_components_values() {
    let path_str = format!("{WASM_TOOLS_TEST_COMP_INPUTS}/values");
    tests_from_wast(Path::new(&path_str), test_event_generation);
}

fn wasm_tools() -> Command {
    Command::new("wasm-tools")
}

pub fn tests_from_wast(path: &Path, run_test: fn(&str)) {
    let path = path.to_str().unwrap().replace("\\", "/");
    for entry in fs::read_dir(path).unwrap() {
        let file = entry.unwrap();
        match file.path().extension() {
            None => continue,
            Some(ext) => {
                if ext.to_str() != Some("wast") {
                    continue;
                }
            }
        }
        let mut cmd = wasm_tools();
        let td = tempfile::TempDir::new().unwrap();
        cmd.arg("json-from-wast")
            .arg(file.path())
            .arg("--pretty")
            .arg("--wasm-dir")
            .arg(td.path())
            .arg("-o")
            .arg(td.path().join(format!(
                "{:?}.json",
                Path::new(&file.path())
                    .file_stem()
                    .unwrap()
                    .to_str()
                    .unwrap()
            )));
        let output = cmd.output().unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            panic!("failed to run {cmd:?}\nstdout: {stdout}\nstderr: {stderr}");
        }
        // For every file that is not invalid in the output, do round-trip
        for entry in fs::read_dir(td.path()).unwrap() {
            let file_json = entry.unwrap();
            match file_json.path().extension() {
                None => continue,
                Some(ext) => {
                    if ext.to_str() != Some("json") {
                        continue;
                    }
                }
            }
            let json: Value = serde_json::from_str(
                &fs::read_to_string(file_json.path()).expect("Unable to open file"),
            )
            .unwrap();
            if let Value::Object(map) = json {
                if let Value::Array(vals) = map.get_key_value("commands").unwrap().1 {
                    for value in vals {
                        if let Value::Object(testcase) = value {
                            // If assert is not in the string, that means it is a valid test case
                            if let Value::String(ty) = testcase.get_key_value("type").unwrap().1 {
                                if !ty.contains("assert") && testcase.contains_key("filename") {
                                    if let Value::String(test_file) =
                                        testcase.get_key_value("filename").unwrap().1
                                    {
                                        run_test(
                                            Path::new(td.path()).join(test_file).to_str().unwrap(),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
