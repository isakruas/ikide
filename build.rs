// Copyright 2026 The IKIDE Authors.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

// Embed the ik8b std library into the IDE binary.
//
// The distributed binary has no `tools/ik8b/std` tree next to it, but the
// in-process compiler resolves `import std/...` from the filesystem. So we
// bake every std `.ik` source into the binary here; at startup the IDE writes
// them next to the executable (see `analysis::ensure_std_dir`).

use std::env;
use std::fs;
use std::path::Path;

fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let std_dir = Path::new(&manifest).join("tools/ik8b/std");
    let out_dir = env::var("OUT_DIR").expect("OUT_DIR");
    let dest = Path::new(&out_dir).join("std_embed.rs");

    // Collect every std/*.ik file (flat directory) as (file_name, abs_path).
    let mut entries: Vec<(String, String)> = Vec::new();
    if let Ok(read) = fs::read_dir(&std_dir) {
        for e in read.flatten() {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("ik") {
                let name = p.file_name().unwrap().to_string_lossy().into_owned();
                entries.push((name, p.to_string_lossy().into_owned()));
            }
        }
    }
    entries.sort();

    // Generate a static table that `include_str!`s each source into the binary.
    let mut code = String::from("pub static STD_FILES: &[(&str, &str)] = &[\n");
    for (name, abspath) in &entries {
        code.push_str(&format!("    ({:?}, include_str!({:?})),\n", name, abspath));
    }
    code.push_str("];\n");
    fs::write(&dest, code).expect("write std_embed.rs");

    let examples_dir = Path::new(&manifest).join("assets/examples");
    let dest_examples = Path::new(&out_dir).join("examples_embed.rs");

    let mut examples_code = String::new();
    examples_code.push_str("#[allow(dead_code)]\n");
    examples_code.push_str("pub struct EmbeddedExampleFile {\n");
    examples_code.push_str("    pub name: &'static str,\n");
    examples_code.push_str("    pub content: &'static str,\n");
    examples_code.push_str("}\n\n");
    examples_code.push_str("#[allow(dead_code)]\n");
    examples_code.push_str("pub struct EmbeddedExample {\n");
    examples_code.push_str("    pub name: &'static str,\n");
    examples_code.push_str("    pub files: &'static [EmbeddedExampleFile],\n");
    examples_code.push_str("}\n\n");
    examples_code.push_str("pub static EMBEDDED_EXAMPLES: &[EmbeddedExample] = &[\n");

    if let Ok(read_dirs) = fs::read_dir(&examples_dir) {
        let mut dirs: Vec<_> = read_dirs.flatten().filter(|e| e.path().is_dir()).collect();
        dirs.sort_by_key(|d| d.file_name());

        for d in dirs {
            let example_name = d.file_name().to_string_lossy().into_owned();
            examples_code.push_str("    EmbeddedExample {\n");
            examples_code.push_str(&format!("        name: {:?},\n", example_name));
            examples_code.push_str("        files: &[\n");

            if let Ok(read_files) = fs::read_dir(d.path()) {
                let mut files: Vec<_> = read_files.flatten().filter(|e| e.path().is_file()).collect();
                files.sort_by_key(|f| f.file_name());

                for f in files {
                    let file_name = f.file_name().to_string_lossy().into_owned();
                    let abs_path = f.path().to_string_lossy().into_owned();
                    examples_code.push_str("            EmbeddedExampleFile {\n");
                    examples_code.push_str(&format!("                name: {:?},\n", file_name));
                    examples_code.push_str(&format!("                content: include_str!({:?}),\n", abs_path));
                    examples_code.push_str("            },\n");
                }
            }
            examples_code.push_str("        ],\n");
            examples_code.push_str("    },\n");
        }
    }
    examples_code.push_str("];\n");
    fs::write(&dest_examples, examples_code).expect("write examples_embed.rs");

    println!("cargo:rerun-if-changed={}", std_dir.display());
    println!("cargo:rerun-if-changed={}", examples_dir.display());
}
