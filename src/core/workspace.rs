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

use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct FileNode {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    pub children: Vec<FileNode>,
}

pub fn scan_workspace(dir: &Path) -> FileNode {
    let mut children = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        paths.sort_by_key(|e| e.file_name());
        
        for entry in paths {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            
            if name.starts_with('.') || name == "target" || name == "tools" || name == "avr-vm" {
                continue;
            }
            
            if path.is_dir() {
                let dir_node = scan_workspace(&path);
                children.push(dir_node);
            } else if path.is_file() {
                // Now allow any files since they requested Create/Rename/Delete
                children.push(FileNode {
                    path,
                    name,
                    is_dir: false,
                    children: Vec::new(),
                });
            }
        }
    }
    
    children.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name))
    });
    
    let name = dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "Workspace".to_string());
        
    FileNode {
        path: dir.to_path_buf(),
        name,
        is_dir: true,
        children,
    }
}
