#![allow(dead_code)]
use log::{error, trace};
use serde_json::Value;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::Command;

pub const WASM_OUTPUT_DIR: &str = "output/wasm";
pub const WAT_OUTPUT_DIR: &str = "output/wat";

/// create output path if it doesn't exist
pub fn ensure_containing_dir(path: impl AsRef<Path>) {
    if !path.as_ref().exists() {
        fs::create_dir_all(path.as_ref().to_path_buf().parent().unwrap()).unwrap();
    }
}

/// Write bytes to a given path on disk
pub fn write_to_file(bytes: &[u8], path: impl AsRef<Path>) {
    ensure_containing_dir(&path);
    let mut file = match File::create(path) {
        Ok(file) => file,
        Err(e) => {
            error!("Failed to create the file: {}", e);
            return;
        }
    };

    // Write the string to the file
    match file.write_all(bytes) {
        Ok(_) => trace!("Data successfully written to the file."),
        Err(e) => error!("Failed to write to the file: {}", e),
    }
}

// ========================
// ==== TEST FRAMEWORK ====
// ========================

pub fn check_instrumentation_encoding(wirm_wat: &String, file: &str) -> Result<(), std::io::Error> {
    let f = File::open(file)?;
    let mut reader = BufReader::new(f);
    let wat_with_instr = get_wat_with_inline_instrumentation(&mut reader)?;
    assert_eq!(*wirm_wat, wat_with_instr);
    Ok(())
}

const INSERT_PREFIX_PATTERN: &str = ";; << ";
const REPLACE_PREFIX_PATTERN: &str = ";; < ";
const REMOVE_PREFIX_PATTERN: &str = ";; rm";
fn get_wat_with_inline_instrumentation(
    reader: &mut BufReader<File>,
) -> Result<String, std::io::Error> {
    let mut wat_with_instr = String::new();

    let mut line = String::new();
    while reader.read_line(&mut line)? > 0 {
        if line.contains(REMOVE_PREFIX_PATTERN) {
            // Do not include
            line.clear();
            continue;
        } else if line.contains(INSERT_PREFIX_PATTERN) {
            // Just insert the code! This should retain indentation
            let new_line = line.replace(INSERT_PREFIX_PATTERN, "");
            wat_with_instr += &new_line;
        } else if line.contains(REPLACE_PREFIX_PATTERN) {
            // Replace the code! Just remove all non-whitespace before and the pattern itself

            // Find the end of the indentation
            let mut end_whitespace_idx = 0;
            for (idx, c) in line.chars().enumerate() {
                if !c.is_whitespace() {
                    end_whitespace_idx = idx;
                    break;
                }
            }
            // Find the beginning of the command
            let command_start = line.find(REPLACE_PREFIX_PATTERN).unwrap();

            // remove original
            line.replace_range(end_whitespace_idx..command_start, "");
            // remove the command
            let new_line = line.replace(REPLACE_PREFIX_PATTERN, "");
            wat_with_instr += &new_line;
        } else {
            wat_with_instr += &line;
        }

        line.clear();
    }
    Ok(
        wasmprinter::print_bytes(wat::parse_str(wat_with_instr.clone()).expect("Error encoding"))
            .unwrap(),
    )
}

fn wasm_tools() -> Command {
    Command::new("wasm-tools")
}

pub fn tests_from_wast(path: &Path) -> Vec<String> {
    let mut tests = vec![];
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
                                        tests.push(
                                            Path::new(td.path())
                                                .join(test_file)
                                                .to_str()
                                                .unwrap()
                                                .parse()
                                                .unwrap(),
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

    tests
}
