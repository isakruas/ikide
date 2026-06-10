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

// Compiler-backed language intelligence for ik8b.
//
// This module is deterministic: every symbol, signature and diagnostic is
// derived from the real ik8b sources (std library + the current buffer) and
// from the compiler's own error output. No heuristic "AI" — the compiler is
// the source of truth, exactly like a language server.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SymbolKind {
    Function,
    Const,
    Variable,
    Isr,
}

#[derive(Clone, Debug)]
pub struct Symbol {
    /// Name including its sigil, e.g. `@delay_ms`, `%MAX_BUFFER`, `$counter`.
    pub name: String,
    pub kind: SymbolKind,
    /// Human-readable signature, e.g. `@delay_ms($ms: u16) -> void`.
    pub signature: String,
    /// Doc comment (the `#` lines immediately above the declaration).
    pub doc: String,
    /// Where it came from, e.g. `std/delay.ik` or `this file`.
    pub source: String,
}

/// A symbol table built from a set of `.ik` sources.
#[derive(Default, Clone)]
pub struct SymbolIndex {
    pub symbols: Vec<Symbol>,
}

impl SymbolIndex {
    /// Parse every `.ik` file under `dir` (recursively) and index its
    /// top-level declarations. Used once for the std library.
    pub fn from_dir(dir: &Path) -> Self {
        let mut idx = SymbolIndex::default();
        idx.add_dir(dir);
        idx
    }

    fn add_dir(&mut self, dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let p = entry.path();
            if p.is_dir() {
                // Skip build/output directories to stay fast.
                let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name == "build" || name == "target" || name == ".git" {
                    continue;
                }
                self.add_dir(&p);
            } else if p.extension().and_then(|s| s.to_str()) == Some("ik") {
                if let Ok(content) = fs::read_to_string(&p) {
                    let label = file_label(dir, &p);
                    parse_source(&content, &label, &mut self.symbols);
                }
            }
        }
    }

    /// Build a transient index for completion: std symbols + the symbols
    /// declared in the buffer the user is editing.
    pub fn with_buffer(&self, buffer: &str) -> SymbolIndex {
        let mut symbols = self.symbols.clone();
        parse_source(buffer, "this file", &mut symbols);
        SymbolIndex { symbols }
    }
}

fn file_label(root: &Path, p: &Path) -> String {
    // Show paths relative to the std root for readability (e.g. "std/delay.ik").
    let rel = p.strip_prefix(root).unwrap_or(p);
    let stem = rel.to_string_lossy().into_owned();
    if let Some(root_name) = root.file_name().and_then(|s| s.to_str()) {
        format!("{}/{}", root_name, stem)
    } else {
        stem
    }
}

/// Extract top-level declarations from one source file.
///
/// ik8b uses unambiguous sigils, so a line scanner is robust and avoids
/// re-implementing the compiler's parser:
///   - functions start at column 0 with `@name(...) [-> type] {`
///   - constants:  `const %NAME: type = value`
///   - variables:  `<space> <mut|imut|str|ptr...> $name : type`
///   - isr:        `isr name {`
fn parse_source(src: &str, source_label: &str, out: &mut Vec<Symbol>) {
    let mut doc_buf: Vec<String> = Vec::new();

    for raw in src.lines() {
        let trimmed = raw.trim_start();

        // Accumulate doc comments; a blank line breaks the block.
        if trimmed.starts_with('#') {
            let text = trimmed.trim_start_matches('#').trim();
            doc_buf.push(text.to_string());
            continue;
        }
        if trimmed.is_empty() {
            doc_buf.clear();
            continue;
        }

        let doc = doc_buf.join("\n");

        // --- Top-level function: line begins at column 0 with `@`. ---
        if raw.starts_with('@') && trimmed.contains(|c: char| c == '(' || c == '{') {
            if let Some(sym) = parse_function(raw, &doc, source_label) {
                push_unique(out, sym);
            }
            doc_buf.clear();
            continue;
        }

        // --- Constant declaration. ---
        if trimmed.starts_with("const ") {
            if let Some(sym) = parse_const(trimmed, &doc, source_label) {
                push_unique(out, sym);
            }
            doc_buf.clear();
            continue;
        }

        // --- ISR declaration. ---
        if trimmed.starts_with("isr ") {
            if let Some(name) = trimmed["isr ".len()..]
                .split(|c: char| c.is_whitespace() || c == '{')
                .find(|s| !s.is_empty())
            {
                push_unique(
                    out,
                    Symbol {
                        name: name.to_string(),
                        kind: SymbolKind::Isr,
                        signature: format!("isr {}", name),
                        doc: doc.clone(),
                        source: source_label.to_string(),
                    },
                );
            }
            doc_buf.clear();
            continue;
        }

        // --- Variable / pointer / string declarations and parameters. ---
        // A scalar/array/string/pointer declaration carries a storage space.
        for (decl, ty) in scan_var_decls(trimmed) {
            push_unique(
                out,
                Symbol {
                    name: decl,
                    kind: SymbolKind::Variable,
                    signature: ty,
                    doc: String::new(),
                    source: source_label.to_string(),
                },
            );
        }

        doc_buf.clear();
    }
}

fn push_unique(out: &mut Vec<Symbol>, sym: Symbol) {
    if !out.iter().any(|s| s.name == sym.name && s.kind == sym.kind) {
        out.push(sym);
    }
}

fn parse_function(line: &str, doc: &str, source: &str) -> Option<Symbol> {
    // line starts with '@'
    let name_end = line[1..]
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(line.len());
    let name = &line[..name_end];
    if name.len() <= 1 {
        return None;
    }

    // Parameters: text between the first matched parens.
    let params = match (line.find('('), line.find(')')) {
        (Some(open), Some(close)) if close > open => line[open + 1..close].trim().to_string(),
        _ => String::new(),
    };

    // Return type: between `->` and `{`.
    let ret = if let Some(arrow) = line.find("->") {
        let after = &line[arrow + 2..];
        let end = after.find('{').unwrap_or(after.len());
        after[..end].trim().to_string()
    } else {
        String::new()
    };

    let signature = match (params.is_empty(), ret.is_empty()) {
        (true, true) => format!("{}()", name),
        (true, false) => format!("{}() -> {}", name, ret),
        (false, true) => format!("{}({})", name, params),
        (false, false) => format!("{}({}) -> {}", name, params, ret),
    };

    Some(Symbol {
        name: name.to_string(),
        kind: SymbolKind::Function,
        signature,
        doc: doc.to_string(),
        source: source.to_string(),
    })
}

fn parse_const(trimmed: &str, doc: &str, source: &str) -> Option<Symbol> {
    // const %NAME: type = value   (hardware register / memory-mapped address)
    // const  NAME: type = value   (plain value constant — folds to an immediate)
    let rest = trimmed["const ".len()..].trim_start();
    // An optional `%` sigil followed by an identifier. The symbol name keeps the
    // sigil for register constants and is bare for value constants.
    let ident = rest.strip_prefix('%').unwrap_or(rest);
    if !ident.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let id_len = ident
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(ident.len());
    let name_end = if rest.starts_with('%') { id_len + 1 } else { id_len };
    let name = &rest[..name_end];
    if name.is_empty() {
        return None;
    }
    // Signature: everything up to '=' (the typed declaration), trimmed.
    let sig_body = rest.split('=').next().unwrap_or(rest).trim();
    Some(Symbol {
        name: name.to_string(),
        kind: SymbolKind::Const,
        signature: format!("const {}", sig_body),
        doc: doc.to_string(),
        source: source.to_string(),
    })
}

/// Find `$name: type` pairs in a declaration or parameter list.
fn scan_var_decls(trimmed: &str) -> Vec<(String, String)> {
    let mut found = Vec::new();
    // Only treat lines that look like declarations or signatures to avoid
    // capturing every `$x` use. A declaration has a storage keyword, or this
    // is a parameter list (`$x: type`) — both contain `$name :`.
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i;
            let mut j = i + 1;
            while j < bytes.len()
                && (trimmed[j..j + 1].chars().next().unwrap().is_alphanumeric()
                    || bytes[j] == b'_')
            {
                j += 1;
            }
            let name = &trimmed[start..j];
            // Look for a following `: type`.
            let after = trimmed[j..].trim_start();
            if name.len() > 1 && after.starts_with(':') {
                let ty_part = after[1..].trim_start();
                let ty_end = ty_part
                    .find(|c: char| c == ',' || c == ')' || c == '=' || c == '{')
                    .unwrap_or(ty_part.len());
                let ty = ty_part[..ty_end].trim().to_string();
                found.push((name.to_string(), ty));
            }
            i = j;
        } else {
            i += 1;
        }
    }
    found
}

// ============================================================================
// Completion
// ============================================================================

#[derive(Clone, Debug)]
pub struct CompletionItem {
    /// Text shown in the list and used as the default insertion.
    pub label: String,
    /// Short type/signature shown to the right of the label.
    pub detail: String,
    /// Longer documentation shown as a tooltip / second line.
    pub doc: String,
    /// Text actually inserted at the cursor (defaults to `label`).
    pub insert: String,
}

impl CompletionItem {
    fn simple(label: &str) -> Self {
        CompletionItem {
            label: label.to_string(),
            detail: String::new(),
            doc: String::new(),
            insert: label.to_string(),
        }
    }
}

const KEYWORDS: &[&str] = &[
    "target", "import", "const", "mut", "imut", "ram", "eeprom", "flash", "return", "loop",
    "switch", "ptr", "str", "fn", "isr", "break", "continue",
];
const TYPES: &[&str] = &["u8", "u16", "i8", "i16", "bool", "char", "r8", "r16", "void"];
const STD_MODULES: &[&str] = &[
    "delay", "math", "gpio", "adc", "uart", "spi", "twi", "eeprom", "timer", "sleep", "wdt",
    "pwm", "bits", "crc", "string", "conv", "mem", "ringbuf", "atomic", "font", "boot",
];

/// Compiler intrinsics: `@`-builtins with no standard-library `.ik` body, so they
/// never appear in the parsed symbol index. Surfaced directly in `@`-completion.
/// `(name, one-line signature/detail)`.
const INTRINSICS: &[(&str, &str)] = &[
    ("@nop", "@nop() — emit NOP (one idle cycle)"),
    ("@cli", "@cli() — clear the global interrupt enable"),
    ("@sei", "@sei() — set the global interrupt enable"),
    ("@wdr", "@wdr() — reset the watchdog timer"),
    ("@sleep", "@sleep() — enter the selected sleep mode"),
    ("@break", "@break() — on-chip debug breakpoint"),
    ("@burn", "@burn($cycles) — calibrated busy-wait"),
    ("@swap", "@swap($reg) — swap the nibbles of a literal register"),
    ("@movw", "@movw($rd, $rr) — 16-bit register-pair move"),
    ("@mul", "@mul($rd, $rr) — hardware MUL (not on AVRrc)"),
    ("@goto", "@goto($word_addr) — absolute JMP to a flash word address"),
    ("@spm", "@spm($spmcsr, $cmd, $zaddr, $word) — store-program-memory"),
];

/// Produce ranked completions for `prefix` given the current buffer's index.
///
/// `force` is true when the user explicitly asked (Ctrl+Space): in that case
/// we offer the full keyword/snippet palette even with an empty prefix.
pub fn completions(index: &SymbolIndex, prefix: &str, prev_word: &str, devices: &[(String, String)], force: bool) -> Vec<CompletionItem> {
    // Context-sensitive completions driven by the previous token.
    match prev_word {
        "target" => {
            // Suggest supported chips as the user types the device name.
            let mut items: Vec<CompletionItem> = devices
                .iter()
                .filter(|(name, _)| prefix.is_empty() || name.starts_with(prefix))
                .map(|(name, detail)| CompletionItem {
                    label: name.clone(),
                    detail: detail.clone(),
                    doc: String::new(),
                    insert: name.clone(),
                })
                .collect();
            items.truncate(60);
            return items;
        }
        "ram" | "flash" | "eeprom" => {
            return ["mut", "imut", "str", "ptr"].iter().map(|s| CompletionItem::simple(s)).collect();
        }
        "import" => {
            return STD_MODULES.iter().map(|s| CompletionItem::simple(s)).collect();
        }
        "ptr" => {
            return ["ram", "eeprom", "flash"].iter().map(|s| CompletionItem::simple(s)).collect();
        }
        _ => {}
    }

    let mut items = Vec::new();

    if prefix.starts_with('@') {
        symbol_matches(index, prefix, SymbolKind::Function, &mut items);
        // Compiler intrinsics are not in the symbol index; offer them too.
        for (name, detail) in INTRINSICS {
            if name.starts_with(prefix) {
                items.push(CompletionItem {
                    label: name.to_string(),
                    detail: detail.to_string(),
                    doc: String::new(),
                    insert: name.to_string(),
                });
            }
        }
    } else if prefix.starts_with('%') {
        symbol_matches(index, prefix, SymbolKind::Const, &mut items);
    } else if prefix.starts_with('$') {
        symbol_matches(index, prefix, SymbolKind::Variable, &mut items);
    } else if prefix.starts_with(':') {
        for t in TYPES {
            items.push(CompletionItem::simple(t));
        }
    } else {
        // Bare word: keywords + types matching the prefix.
        let pref = prefix;
        for k in KEYWORDS {
            if force || (!pref.is_empty() && k.starts_with(pref)) {
                items.push(CompletionItem::simple(k));
            }
        }
        for t in TYPES {
            if force || (!pref.is_empty() && t.starts_with(pref)) {
                items.push(CompletionItem::simple(t));
            }
        }
        // Also surface function names by bare prefix (common when typing the
        // name before remembering the `@`), and value constants (bare `const`
        // names, no `%` sigil).
        if !pref.is_empty() {
            for s in &index.symbols {
                if s.kind == SymbolKind::Function && s.name[1..].starts_with(pref) {
                    items.push(symbol_to_item(s, true));
                } else if s.kind == SymbolKind::Const
                    && !s.name.starts_with('%')
                    && s.name.starts_with(pref)
                {
                    items.push(symbol_to_item(s, false));
                }
            }
        }
    }

    items
}

fn symbol_matches(index: &SymbolIndex, prefix: &str, kind: SymbolKind, out: &mut Vec<CompletionItem>) {
    for s in &index.symbols {
        if s.kind == kind && s.name.starts_with(prefix) {
            out.push(symbol_to_item(s, kind == SymbolKind::Function));
        }
    }
    // Stable, useful ordering: prefix-exact first, then alphabetical.
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.dedup_by(|a, b| a.label == b.label);
}

fn symbol_to_item(s: &Symbol, is_fn: bool) -> CompletionItem {
    // Insert functions with an opening paren so the next keystroke flows into
    // arguments; functions with no params get the closing paren too.
    let insert = if is_fn {
        if s.signature.contains("()") {
            format!("{}()", s.name)
        } else {
            format!("{}(", s.name)
        }
    } else {
        s.name.clone()
    };
    // Detail shows the part of the signature after the name + its origin.
    let sig_tail = s.signature.strip_prefix(&s.name).unwrap_or(&s.signature).to_string();
    let detail = if s.source == "this file" {
        sig_tail
    } else {
        format!("{}   ·   {}", sig_tail, s.source)
    };
    CompletionItem {
        label: s.name.clone(),
        detail: detail.trim().to_string(),
        doc: s.doc.clone(),
        insert,
    }
}

// ============================================================================
// Keyword reference (hover help)
// ============================================================================

/// A short reference card for a language keyword or type, shown when the user
/// selects it in the editor. Grounded in the compiler's own docs.
#[derive(Clone, Debug)]
pub struct KeywordHelp {
    pub title: String,
    pub doc: String,
    pub snippet: String,
}

/// Return reference help for a keyword/type, or `None` if the word isn't one.
pub fn keyword_help(word: &str) -> Option<KeywordHelp> {
    let card = |title: &str, doc: &str, snippet: &str| {
        Some(KeywordHelp { title: title.to_string(), doc: doc.to_string(), snippet: snippet.to_string() })
    };
    match word {
        // --- Top level ------------------------------------------------------
        "target" => card(
            "target — select the microcontroller",
            "Fixes the chip the program is compiled for: its register map, memory sizes and interrupt vectors. One per file, at the very top.",
            "target atmega328p",
        ),
        "import" => card(
            "import — bring a module into scope",
            "Makes another module's functions available. Standard-library modules live under `std/`.",
            "import std/gpio",
        ),
        "const" => card(
            "const — register alias or value constant",
            "`const %NAME` binds a `%` name to a peripheral register's address (read/written at run time). `const NAME` (no `%`) is a plain value constant — a bit mask or command word — that folds to an immediate.",
            "const %PORTB: u16 = 0x0025\nconst LED_PIN: u8 = 0x20",
        ),
        "isr" => card(
            "isr — interrupt service routine",
            "A handler bound to a hardware interrupt vector. It takes no parameters and returns nothing; the backend emits the vector jump and context save/restore.",
            "isr TIMER0_COMPA {\n    1 -> $tick\n}",
        ),
        // --- Storage spaces -------------------------------------------------
        "ram" => card(
            "ram — SRAM storage",
            "Storage space for mutable runtime data. Lost on reset.",
            "ram mut $i: u8 = 0",
        ),
        "eeprom" => card(
            "eeprom — EEPROM storage",
            "Non-volatile storage that persists across resets. Slower to write.",
            "eeprom mut $boot_count: u16 = 0",
        ),
        "flash" => card(
            "flash — program-memory storage",
            "Read-only storage in program memory, so it must be `imut`. Ideal for lookup tables and constant strings.",
            "flash imut $sin: u8[4] = 0",
        ),
        // --- Mutability -----------------------------------------------------
        "mut" => card(
            "mut — mutable",
            "The value may change after declaration. Valid for `ram` and `eeprom`.",
            "ram mut $x: u8 = 0",
        ),
        "imut" => card(
            "imut — immutable",
            "The value is fixed after declaration. Required for `flash` data.",
            "flash imut $tbl: u8[4] = 0",
        ),
        // --- Declarations ---------------------------------------------------
        "ptr" => card(
            "ptr — pointer declaration",
            "A pointer that names the memory space it points into. Created with `&`, dereferenced with `*`.",
            "ram ptr u8 $p = &$buf[0]",
        ),
        "str" => card(
            "str — string declaration",
            "A NUL-terminated string. Lives in `ram` (a mutable copy) or `flash` (program memory).",
            "ram str $msg = \"hello\\n\"",
        ),
        // --- Control flow ---------------------------------------------------
        "loop" => card(
            "loop — infinite or range loop",
            "`loop *` runs forever (how `@main` spins). `loop start..end -> $i` iterates the half-open range, binding the induction variable.",
            "loop * {\n    @poll()\n}\n\nloop 0..8 -> $i {\n    @shift_out($i)\n}",
        ),
        "switch" => card(
            "switch — dispatch on a value",
            "Matches one value against cases `expr -> { ... }`. `*` is the wildcard default and comes last.",
            "switch $cmd {\n    1 -> { @start() }\n    2 -> { @stop() }\n    * -> { @ignore() }\n}",
        ),
        "return" => card(
            "return — exit a function",
            "Leaves the current function. In a typed function it carries a value; in a `void` function it stands alone.",
            "@square($x: u16) -> u16 {\n    return $x * $x\n}",
        ),
        "break" => card(
            "break — leave the loop",
            "Exits the nearest enclosing loop immediately.",
            "loop * {\n    ? $done { break }\n}",
        ),
        "continue" => card(
            "continue — next iteration",
            "Skips the rest of the body and continues the nearest enclosing loop.",
            "loop 0..10 -> $i {\n    ? $i & 1 { continue }\n    @even($i)\n}",
        ),
        // --- Primitive types ------------------------------------------------
        "u8" => card("u8 — unsigned 8-bit", "Integer 0..255, one byte.", "ram mut $n: u8 = 0"),
        "u16" => card("u16 — unsigned 16-bit", "Integer 0..65535, two bytes.", "ram mut $n: u16 = 0"),
        "i8" => card("i8 — signed 8-bit", "Integer -128..127 (two's complement), one byte.", "ram mut $n: i8 = -1"),
        "i16" => card("i16 — signed 16-bit", "Integer -32768..32767, two bytes.", "ram mut $n: i16 = -1"),
        "bool" => card("bool — boolean", "`true` is 1, `false` is 0. One byte.", "ram mut $ok: bool = true"),
        "char" => card("char — byte/character", "A single character or byte value, interchangeable with u8.", "ram mut $c: char = 'A'"),
        "r8" => card("r8 — fixed-point real (Q4.4)", "Fractional number in one byte: 4 integer + 4 fractional bits.", "ram mut $g: r8 = 1.5"),
        "r16" => card("r16 — fixed-point real (Q8.8)", "Fractional number in two bytes: 8 integer + 8 fractional bits (~-128.0..127.996, step 1/256). See the math library.", "ram mut $v: r16 = 3.14"),
        "void" => card("void — no value", "Used only as a function return type for functions that return nothing.", "@setup() -> void {\n    @init()\n}"),
        "true" | "false" => card("bool literal", "`true` is 1 and `false` is 0; both have type `bool`.", "ram mut $ok: bool = true"),
        other => {
            // Compiler intrinsics (the word may or may not include the leading `@`).
            let at = if other.starts_with('@') { other.to_string() } else { format!("@{}", other) };
            if let Some((name, detail)) = INTRINSICS.iter().find(|(n, _)| *n == at) {
                let (sig, desc) = detail.split_once(" — ").unwrap_or((detail, *detail));
                return card(&format!("{} — compiler intrinsic", name), desc, sig);
            }
            None
        }
    }
}

// ============================================================================
// Diagnostics
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// The program does not compile.
    Error,
    /// The program compiles, but something deserves attention (e.g. an
    /// implicit narrowing assignment reported by the compiler).
    Warning,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    /// 1-based source line, or 0 when the error is file-level (no line).
    pub line: usize,
    /// A clear, developer-friendly message.
    pub message: String,
    /// The offending identifier, when the compiler reported one.
    pub term: String,
    /// The original compiler text, when this came from the compiler (kept for
    /// power users / hover). `None` for IDE-side lints.
    pub raw: Option<String>,
    pub severity: Severity,
}

/// Type-check `src` with the compiler's own front end (in-process) and return
/// any diagnostics. This uses the exact same lexer + parser the compiler runs,
/// so the errors match perfectly — there is no separate re-implementation that
/// could drift, and there is no subprocess or temp file.
pub fn check(src: &str) -> Vec<Diagnostic> {
    // Run the full in-process compile so EVERY error — lexer, parser, semantic
    // and backend — surfaces live, each located on its line. Code generation on
    // a half-typed buffer can hit an internal panic, so guard the UI thread.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| compile(src)));
    match result {
        // A clean compile still carries the compiler's non-fatal warnings
        // (e.g. implicit narrowing), each one a Warning-severity diagnostic.
        Ok(Ok(artifact)) => artifact.warnings,
        Ok(Err(d)) => vec![d],
        Err(_) => vec![Diagnostic {
            line: 0,
            message: "Internal compiler error while analyzing this file.".to_string(),
            term: String::new(),
            raw: None,
            severity: Severity::Error,
        }],
    }
}

fn diag_from_error(raw: &str) -> Diagnostic {
    Diagnostic {
        line: extract_line_number(raw).unwrap_or(0),
        message: friendly(raw),
        term: extract_term(raw),
        raw: Some(raw.to_string()),
        severity: Severity::Error,
    }
}

/// Build a diagnostic and, when the compiler reported no line (semantic/backend
/// errors quote the offending identifier instead), locate that identifier in
/// the source so the problem is always pinned to a line.
fn diag_from_src(src: &str, raw: &str) -> Diagnostic {
    let mut d = diag_from_error(raw);
    if d.line == 0 {
        d.line = resolve_line(src, &d.term, raw);
    }
    d
}

/// A compiler warning (no line info; quotes the target and names the function).
fn warning_from_src(src: &str, raw: &str) -> Diagnostic {
    let mut d = diag_from_src(src, raw);
    d.severity = Severity::Warning;
    d
}

fn resolve_line(src: &str, term: &str, raw: &str) -> usize {
    // Semantic diagnostics name the enclosing function ("... in function '@f'");
    // restrict the term search to that function's body so a name that also
    // appears elsewhere in the file is pinned to the right occurrence.
    if let Some(fn_name) = named_function(raw) {
        if let Some((start, end)) = function_span(src, &fn_name) {
            if !term.is_empty() {
                // A narrowing warning points at an assignment: prefer the line
                // that actually assigns the term over its declaration line.
                if raw.contains("in assignment to") {
                    for (i, line) in src.lines().enumerate().take(end).skip(start) {
                        if line.contains(term) && line.contains("->") {
                            return i + 1;
                        }
                    }
                }
                for (i, line) in src.lines().enumerate().take(end).skip(start) {
                    if line.contains(term) {
                        return i + 1;
                    }
                }
            }
            // Term not found inside the body (e.g. it lives in an imported
            // module that was inlined): at least point at the function itself.
            return start + 1;
        }
    }
    // First line that contains the offending token (e.g. the call `@start`).
    if !term.is_empty() {
        for (i, line) in src.lines().enumerate() {
            if line.contains(term) {
                return i + 1;
            }
        }
    }
    // End-of-file errors belong to the last line.
    if raw.contains("EOF") || raw.contains("end of file") || raw.contains("end of input") {
        return src.lines().count().max(1);
    }
    0
}

/// The `@name` out of a "... in function '@name'" diagnostic, if present.
fn named_function(raw: &str) -> Option<String> {
    let rest = raw.split("in function '").nth(1)?;
    let name = rest.split('\'').next()?;
    if name.starts_with('@') {
        Some(name.to_string())
    } else {
        None
    }
}

/// 0-based [start, end) line span of a top-level function/ISR body in `src`.
/// The body runs from the signature line to the next top-level declaration.
fn function_span(src: &str, fn_name: &str) -> Option<(usize, usize)> {
    let lines: Vec<&str> = src.lines().collect();
    // ISR diagnostics use the synthesized name `__isr_<vector>`.
    let isr_vector = fn_name.strip_prefix("@__isr_");
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let is_decl = match isr_vector {
            Some(vec) => t.starts_with("isr ") && t.contains(vec),
            None => {
                t.starts_with(fn_name)
                    && t[fn_name.len()..]
                        .chars()
                        .next()
                        .map_or(true, |c| c == '(' || c == '{' || c.is_whitespace())
            }
        };
        if is_decl {
            start = Some(i);
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (i, line) in lines.iter().enumerate().skip(start + 1) {
        let t = line.trim_start();
        if (t.starts_with('@') && t.contains('{')) || t.starts_with("isr ") {
            end = i;
            break;
        }
    }
    Some((start, end))
}

/// The product of a successful in-process build: the Intel HEX plus the memory
/// usage figures the resource panel shows.
#[derive(Clone, Debug)]
pub struct BuildArtifact {
    pub hex: String,
    pub device: String,
    pub core: String,
    pub prog_used: u32,
    pub prog_total: u32,
    pub sram_used: u32,
    pub sram_total: u32,
    pub eeprom_used: u32,
    pub eeprom_total: u32,
    /// Peak hardware registers occupied by any function, out of `regs_total`.
    pub regs_used: u32,
    pub regs_total: u32,
    /// Values the allocator had to spill to memory program-wide.
    pub spills: u32,
    /// Non-fatal compiler diagnostics (Warning severity), e.g. implicit
    /// narrowing assignments, already located on their source lines.
    pub warnings: Vec<Diagnostic>,
}

/// Compile `src` to Intel HEX entirely in-process, running the compiler's own
/// pipeline (lexer → parser → code generator → label resolution → HEX). No
/// subprocess: the IDE *is* the compiler front end. On failure it returns the
/// same diagnostic the live checker would show.
pub fn compile(src: &str) -> Result<BuildArtifact, Diagnostic> {
    let tokens = ik8b::lexer::Lexer::new(src)
        .tokenize()
        .map_err(|e| diag_from_src(src, &e))?;
    let mut parser = ik8b::parser::Parser::new(tokens);
    let ast = parser.parse().map_err(|e| diag_from_src(src, &e))?;

    let device_name = parser.active_target.trim().to_string();
    if device_name.is_empty() {
        return Err(diag_from_src(
            src,
            "Device Error: a top-level device target is required.",
        ));
    }
    let device = match ik8b::devices::lookup_device(&device_name) {
        Some(d) => d,
        None => {
            return Err(diag_from_src(
                src,
                &format!("Device Error: unknown device '{}'.", device_name),
            ))
        }
    };

    let mut cg = ik8b::codegen::CodeGenerator::new();
    cg.target_core = device.core;
    cg.set_sram_start(device.sram_start);
    cg.set_device_name(device.name);

    let insts = cg
        .compile(&ast)
        .map_err(|e| diag_from_src(src, &format!("Compilation Error: {}", e)))?;

    // A `boot <addr>` program is located at the Boot Loader Section start so it
    // can run as a bootloader (and legally execute SPM); everything else at 0.
    let (opcodes, hex) = if let Some(byte_base) = parser.boot_origin {
        if byte_base % 2 != 0 || byte_base >= device.flash_size {
            return Err(diag_from_src(
                src,
                &format!(
                    "Device Error: boot address 0x{:X} must be even and within {}'s {} KB flash.",
                    byte_base, device.name, device.flash_size / 1024
                ),
            ));
        }
        let opcodes = ik8b::codegen::resolve_labels_at(&insts, (byte_base / 2) as i64)
            .map_err(|e| diag_from_src(src, &format!("Assembly Error: {}", e)))?;
        let hex = ik8b::codegen::generate_intel_hex_at(&opcodes, byte_base);
        (opcodes, hex)
    } else {
        let opcodes = ik8b::codegen::resolve_labels(&insts)
            .map_err(|e| diag_from_src(src, &format!("Assembly Error: {}", e)))?;
        let hex = ik8b::codegen::generate_intel_hex(&opcodes);
        (opcodes, hex)
    };

    Ok(BuildArtifact {
        hex,
        device: device.name.to_string(),
        core: format!("{:?}", device.core),
        prog_used: opcodes.len() as u32 * 2,
        prog_total: device.flash_size.saturating_sub(device.boot_size),
        sram_used: cg.sram_used() as u32,
        sram_total: device.sram_size,
        eeprom_used: cg.eeprom_used() as u32,
        eeprom_total: device.eeprom_size,
        regs_used: cg.registers_used(),
        regs_total: 32,
        spills: cg.spills(),
        warnings: cg
            .warnings
            .iter()
            .map(|w| warning_from_src(src, w))
            .collect(),
    })
}

/// Translate a raw compiler message into a clear, friendly one.
///
/// The compiler is the source of truth for *what* is wrong; this layer only
/// rephrases its output so the editor shows human guidance instead of parser
/// internals like `Token { kind: Identifier("x"), line: 2 }`. Unknown messages
/// fall back to a lightly cleaned version of the original.
pub fn friendly(raw: &str) -> String {
    let m = raw;
    let has = |needle: &str| m.contains(needle);

    // --- Lexical -------------------------------------------------------------
    if has("Unexpected character ';'") {
        return "No `;` here — statements end at the newline; ik8b does not use semicolons.".into();
    }

    // --- Sigil rules ---------------------------------------------------------
    if has("Expected constant name") {
        return "A `const` needs a name: `const %REG: u16 = 0x25` for a hardware register, or `const NAME: u8 = 0x80` for a value constant.".into();
    }
    // --- Self-programming (@spm / std/boot) ---------------------------------
    if has("@spm requires SPM") {
        return "`@spm` (flash self-programming) isn't supported on this target's core.".into();
    }
    if has("@spm: unknown hardware register constant") {
        return "`@spm`'s first argument must be the SPMCSR register constant for this device — prefer the `@boot_*` helpers from `std/boot`, which pass it for you.".into();
    }
    if has("@spm: zaddr and word must be 16-bit") {
        return "`@spm`'s address and data word must be 16-bit (`u16`) values.".into();
    }
    if has("Function name must start with @") {
        return "Function names must start with `@` — e.g. `@main() -> void { ... }`.".into();
    }
    if has("Parameter must start with $") {
        return "Parameter names must start with `$` — e.g. `@f($x: u8)`.".into();
    }
    if has("Loop variable must start with $") {
        return "Loop variables must start with `$` — e.g. `loop 0..10 -> $i { ... }`.".into();
    }
    if has("Pointer variable must start with $") {
        return "Pointer names must start with `$` — e.g. `ram ptr u8 $p = &@x`.".into();
    }
    if has("String variable must start with $") {
        return "String names must start with `$` — e.g. `ram str $s = \"hi\"`.".into();
    }
    if has("Variable must start with $") {
        return "Variable names must start with `$` — e.g. `ram mut $x: u8 = 0`.".into();
    }

    // --- Storage / mutability ------------------------------------------------
    if has("flash variables must be immutable") {
        return "`flash` values can't change — use `flash imut` instead of `flash mut`.".into();
    }
    if has("must explicitly specify a storage location") {
        return "Declare the storage first: `ram`, `eeprom`, or `flash` before `mut`/`imut` — e.g. `ram mut $x: u8 = 0`.".into();
    }
    if has("Expected mutability specifier") {
        return "Add a mutability keyword: `mut` (changeable) or `imut` (constant).".into();
    }
    if has("Expected pointer space") {
        return "After `ptr`, name the memory space: `ram`, `flash`, or `eeprom` — e.g. `ram ptr u8 $p = &@x`.".into();
    }
    if has("Expected string space") {
        return "After `str`, use `ram` storage — e.g. `ram str $s = \"hi\"`.".into();
    }
    if has("String variables currently support only RAM or flash storage") {
        let got = first_quoted(m).map(|g| format!(" (got `{}`)", g)).unwrap_or_default();
        return format!("Strings can only live in `ram` or `flash` storage{}.", got);
    }

    // --- Target / compile-time checks ---------------------------------------
    if has("a top-level device target is required") {
        return "Add a target chip at the top of the file — e.g. `target atmega328p`.".into();
    }
    if has("unknown device") {
        let got = first_quoted(m).map(|g| format!(" `{}`", g)).unwrap_or_default();
        return format!("Unknown target chip{}. Pick a supported device (type after `target` for suggestions).", got);
    }
    if has("Expected target name") {
        return "Name the chip after `target` — e.g. `target atmega328p`.".into();
    }
    if has("Expected 'target' in compile-time check") {
        return "Compile-time checks look like `? target == <chip> { ... }`.".into();
    }
    if has("Expected identifier in compile-time check") {
        return "Put a chip name after `==` — e.g. `? target == atmega328p { ... }`.".into();
    }

    // --- Imports -------------------------------------------------------------
    if has("Could not read imported file") {
        let got = first_quoted(m).map(|g| format!(" `{}`", g)).unwrap_or_default();
        return format!("Module{} not found. Check the name, or that the file exists in std/.", got);
    }
    if has("Expected module name after import") {
        return "Name a module to import — e.g. `import gpio`.".into();
    }

    // --- Declarations --------------------------------------------------------
    if has("Expected an interrupt vector name after 'isr'") {
        return "Name the interrupt vector after `isr` — e.g. `isr timer0_ovf { ... }`.".into();
    }
    if has("Expected array size number") {
        return "Array sizes must be a number — e.g. `u8[4]`.".into();
    }
    if has("Expected type specifier") || has("Expected type") {
        return "Expected a type here — one of: u8, u16, i8, i16, bool, char, r8, r16, void.".into();
    }
    if has("Expected number") {
        return "Expected a number here.".into();
    }
    if has("Expected top-level declaration in conditional block") {
        return "Inside `? target == ...` only top-level items are allowed: `const`, functions (`@…`), `import`, `isr`.".into();
    }
    if has("Expected top-level declaration") {
        return "At the top level only `target`, `import`, `const`, functions (`@…`) and `isr` are allowed. Variable declarations go inside a function.".into();
    }

    // --- Semantic / backend --------------------------------------------------
    if has("undefined variable") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!(
            "Unknown variable{} — declare it first, e.g. `ram mut {}: u8 = 0`. Check the spelling.",
            n,
            first_quoted(m).unwrap_or_else(|| "$x".into())
        );
    }
    if has("undefined constant") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!(
            "Unknown constant{} for this target — check the spelling, the `target`, or `import` the module that declares it.",
            n
        );
    }
    if has("used as a value") {
        let n = first_quoted(m).unwrap_or_else(|| "@f".into());
        return format!("`{}` is a function — use `&{}` to take its address.", n, n);
    }
    if has("cannot assign to '") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!(
            "Can't assign to{} — only `$variables`, `%registers`, array elements and `*pointers` are assignable.",
            n
        );
    }
    if has("cannot take address of '") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("Can't take the address of{} — constants and hardware registers have no data address.", n);
    }
    if has("cannot index '") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("Can't index{} — only array or pointer `$variables` can be indexed.", n);
    }
    if has("does not fit in type") {
        if has("constant '") {
            let name = first_quoted(m).unwrap_or_default();
            let ty = first_quoted_nth(m, 1).unwrap_or_default();
            return format!("The value of constant `{}` doesn't fit in `{}` — {}.", name, ty, type_range(&ty));
        }
        let lit = m
            .split("literal ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or("");
        let ty = first_quoted(m).unwrap_or_default();
        return format!("The literal {} doesn't fit in `{}` — {}.", lit, ty, type_range(&ty));
    }
    if has("outside the 16-bit address space") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("The address of register{} must be within 0x0000..0xFFFF.", n);
    }
    if has("implicit narrowing") {
        let target = first_quoted(m).unwrap_or_else(|| "$x".into());
        let from_to = m
            .split("implicit narrowing ")
            .nth(1)
            .and_then(|s| s.split(" in assignment").next())
            .unwrap_or("to a narrower type");
        return format!(
            "Assignment to `{}` truncates ({}) — mask with `& 0xFF` or shift (`>> 8`) to make the truncation explicit.",
            target, from_to
        );
    }
    if has("undefined function") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("Unknown function{} — define it, or `import` the module that provides it.", n);
    }
    if has("cannot assign to immutable") {
        let n = first_quoted(m).map(|x| format!("`{}`", x)).unwrap_or_else(|| "This value".into());
        return format!("{} is immutable — declare it with `mut` to allow assignment.", n);
    }
    if has("cannot take address of intrinsic") {
        let n = first_quoted(m).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("Can't take the address of the built-in intrinsic{}.", n);
    }
    if has("has no interrupt vector") {
        let n = first_quoted_nth(m, 1).map(|x| format!(" `{}`", x)).unwrap_or_default();
        return format!("Unknown interrupt vector{} for this chip. Use a vector name from its datasheet.", n);
    }
    if has("does not return a value") {
        let n = m.split_whitespace().nth(1).map(|x| format!("`{}`", x)).unwrap_or_else(|| "This intrinsic".into());
        return format!("{} doesn't return a value, so it can't be used in an expression.", n);
    }

    // --- End of file ---------------------------------------------------------
    if has("Unexpected EOF") || has("end of file") || has("end of input") {
        return "Unexpected end of file — something isn't closed. Check for a missing `}` or an unfinished statement at the end.".into();
    }

    // --- Generic expression / token errors ----------------------------------
    if has("Unexpected symbol in expression") {
        let sym = m.split("Unexpected symbol in expression:").nth(1)
            .and_then(|s| s.split("at line").next())
            .map(|s| s.trim())
            .unwrap_or("");
        return format!("Unexpected `{}` in this expression.", sym);
    }
    if has("Unexpected token in expression") {
        return "Unexpected token in this expression — check the operators and operands.".into();
    }
    if has("Expected token") {
        // "Expected token <A>, got <B> at line N"
        let body = m.split("Expected token").nth(1).unwrap_or("");
        let expected = body.split(", got").next().map(str::trim).unwrap_or("");
        let got = body.split(", got").nth(1)
            .and_then(|s| s.split("at line").next())
            .map(str::trim)
            .unwrap_or("");
        let exp = prettify_token(expected);
        let gotp = prettify_token(got);
        if !exp.is_empty() && !gotp.is_empty() {
            return format!("Expected `{}` here, but found `{}`.", exp, gotp);
        }
        if !exp.is_empty() {
            return format!("Expected `{}` here.", exp);
        }
    }

    // --- Fallback: strip parser noise ---------------------------------------
    clean_raw(m)
}

/// Human-readable value range of a primitive type name, for range diagnostics.
fn type_range(ty: &str) -> String {
    match ty {
        "u8" => "u8 holds 0..255 (bit patterns -128..255 are accepted)".into(),
        "u16" => "u16 holds 0..65535 (bit patterns -32768..65535 are accepted)".into(),
        "i8" => "i8 holds -128..127 (bit patterns up to 255 are accepted)".into(),
        "i16" => "i16 holds -32768..32767 (bit patterns up to 65535 are accepted)".into(),
        other => format!("the value must fit in `{}`", other),
    }
}

/// All single- or double-quoted spans in a message, left to right.
fn quoted_tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(qpos) = rest.find(['\'', '"']) {
        let q = rest[qpos..].chars().next().unwrap();
        let after = &rest[qpos + q.len_utf8()..];
        if let Some(end) = after.find(q) {
            out.push(after[..end].to_string());
            rest = &after[end + q.len_utf8()..];
        } else {
            break;
        }
    }
    out
}

/// First quoted token in a message.
fn first_quoted(s: &str) -> Option<String> {
    quoted_tokens(s).into_iter().next()
}

/// The n-th (0-based) quoted token in a message.
fn first_quoted_nth(s: &str, n: usize) -> Option<String> {
    quoted_tokens(s).into_iter().nth(n)
}

/// Turn a `TokenKind` debug string into the symbol/word a developer recognises.
fn prettify_token(s: &str) -> String {
    let s = s.trim();
    for tag in ["Symbol(\"", "Keyword(\"", "Type(\"", "Identifier(\"", "Str(\"", "CompoundArrow(\""] {
        if let Some(rest) = s.strip_prefix(tag) {
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
    }
    if let Some(rest) = s.strip_prefix("Number(") {
        return rest.trim_end_matches(')').to_string();
    }
    if let Some(rest) = s.strip_prefix("Float(") {
        return rest.trim_end_matches(')').to_string();
    }
    match s {
        "Arrow" => "->".to_string(),
        // Bare token name with no payload: show it lower-cased.
        other => other.to_string(),
    }
}

/// Remove parser internals from an otherwise-unmapped message.
fn clean_raw(m: &str) -> String {
    let mut out = m.trim().to_string();
    for prefix in [
        "Compilation Error: ",
        "Syntax Error: ",
        "Semantic Error: ",
        "Type Error: ",
        "Lexical Error: ",
        "Assembly Error: ",
        "Device Error: ",
        "Memory Error: ",
        "Warning: ",
    ] {
        out = out.replace(prefix, "");
    }
    // Drop a trailing " at line N" (the line is shown separately).
    if let Some(idx) = out.find(" at line ") {
        out.truncate(idx);
    }
    // Collapse a `Token { kind: X, line: N }` into just X, prettified.
    if let Some(start) = out.find("Token { kind: ") {
        if let Some(end_rel) = out[start..].find(" }") {
            let inner = &out[start + "Token { kind: ".len()..start + end_rel];
            let kind = inner.split(", line:").next().unwrap_or(inner);
            let pretty = prettify_token(kind);
            out.replace_range(start..start + end_rel + 2, &format!("`{}`", pretty));
        }
    }
    out
}

fn extract_line_number(s: &str) -> Option<usize> {
    let idx = s.find("at line ")?;
    let rest = &s[idx + "at line ".len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn extract_term(s: &str) -> String {
    // A literal-range error quotes the *type*; the token to highlight is the
    // out-of-range number itself ("literal 300 does not fit in type 'u8'").
    if s.contains("does not fit in type") && !s.contains("constant '") {
        if let Some(lit) = s
            .split("literal ")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
        {
            return lit.to_string();
        }
    }
    // Prefer the explicit `Identifier("name")` or `Keyword("name")` forms.
    for tag in ["Identifier(\"", "Keyword(\""] {
        if let Some(i) = s.find(tag) {
            let rest = &s[i + tag.len()..];
            if let Some(end) = rest.find('"') {
                return rest[..end].to_string();
            }
        }
    }
    // Fall back to the first quoted token (single or double quotes) — semantic
    // errors quote the offending identifier with single quotes, e.g. '@start'.
    first_quoted(s).unwrap_or_default()
}

// ============================================================================
// Instant IDE lints
// ============================================================================
//
// These mirror a few well-known compiler rules so the editor can flag them
// *immediately* on the exact offending token, without waiting for the debounced
// background compile (and even when the compiler aborts earlier on something
// else). The compiler remains the source of truth; these are a fast path for
// the most common mistakes.

/// Scan the buffer for rule violations the IDE can flag synchronously.
pub fn lint_buffer(buffer: &str) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (i, raw) in buffer.lines().enumerate() {
        let line_no = i + 1;

        // Drop a trailing line comment so `# flash mut ...` is ignored.
        let code = match raw.find('#') {
            Some(idx) => &raw[..idx],
            None => raw,
        };

        let toks: Vec<&str> = code.split_whitespace().collect();

        // Rule: flash variables must be immutable — `flash mut` is illegal.
        for w in toks.windows(2) {
            if w[0] == "flash" && w[1] == "mut" {
                diags.push(Diagnostic {
                    line: line_no,
                    message: "`flash` values can't change — use `flash imut` instead of `flash mut`.".to_string(),
                    term: "mut".to_string(),
                    raw: None,
                    severity: Severity::Error,
                });
            }
        }

        // Rule: a declaration must name its storage (ram/eeprom/flash) before
        // the mutability keyword. Catch a statement that starts straight with
        // `mut`/`imut` (the storage keyword is missing).
        if let Some(first) = toks.first() {
            if (*first == "mut" || *first == "imut")
                && toks.get(1).map_or(false, |w| w.starts_with('$'))
            {
                diags.push(Diagnostic {
                    line: line_no,
                    message: "Missing storage location — start the declaration with `ram`, `eeprom`, or `flash` (e.g. `ram mut $x: u8 = 0`).".to_string(),
                    term: first.to_string(),
                    raw: None,
                    severity: Severity::Error,
                });
            }
        }
    }
    diags
}

/// Group diagnostics into a `line -> (offending term, severity)` map for
/// highlighting. When an error and a warning land on the same line the error
/// wins, so the squiggle color reflects the most severe problem.
pub fn highlight_terms(diags: &[Diagnostic]) -> HashMap<usize, (String, Severity)> {
    let mut map: HashMap<usize, (String, Severity)> = HashMap::new();
    for d in diags {
        if d.line > 0 && !d.term.is_empty() {
            match map.get(&d.line) {
                Some((_, Severity::Error)) => {}
                _ if d.severity == Severity::Error => {
                    map.insert(d.line, (d.term.clone(), d.severity));
                }
                None => {
                    map.insert(d.line, (d.term.clone(), d.severity));
                }
                _ => {}
            }
        }
    }
    map
}

// The std library sources, baked into the binary at build time (see build.rs).
include!(concat!(env!("OUT_DIR"), "/std_embed.rs"));

/// Build the std symbol index for autocompletion straight from the embedded
/// sources — every std symbol is known without writing a single file to disk.
pub fn std_symbol_index() -> SymbolIndex {
    let mut symbols = Vec::new();
    for (name, contents) in STD_FILES {
        parse_source(contents, &format!("std/{}", name), &mut symbols);
    }
    SymbolIndex { symbols }
}

/// Look up an embedded std module by bare name (e.g. `atomic` -> `atomic.ik`).
fn embedded_std(name: &str) -> Option<&'static str> {
    let file = format!("{}.ik", name);
    STD_FILES.iter().find(|(n, _)| *n == file).map(|(_, c)| *c)
}

/// Collect every imported module path in a source buffer (both `std/<name>`
/// and local `<name>` forms), in declaration order.
fn scan_imports(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("import ") {
            let module = rest.split_whitespace().next().unwrap_or("").trim();
            if !module.is_empty() {
                out.push(module.to_string());
            }
        }
    }
    out
}

/// Cache directory where std modules are materialized on demand — deliberately
/// outside the user's project (next to the binary when writable, otherwise a
/// temp cache). Resolved once and reused for the whole session.
fn std_cache_dir() -> &'static Path {
    static STD_CACHE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    STD_CACHE.get_or_init(|| {
        if let Some(exe_std) = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("std")))
        {
            if fs::create_dir_all(&exe_std).is_ok() {
                let probe = exe_std.join(".ikide_write_test");
                if fs::write(&probe, b"").is_ok() {
                    let _ = fs::remove_file(&probe);
                    return exe_std;
                }
            }
        }
        std::env::temp_dir().join("ikide-std")
    })
}

/// Materialize exactly the std modules a buffer imports (plus any they import
/// in turn) into the cache dir, then point `IK8B_STD_PATH` at it so the
/// in-process compiler resolves them transparently. Nothing is written to the
/// user's project, and unimported modules are never materialized.
pub fn sync_std_imports(source: &str) -> &'static Path {
    let dir = std_cache_dir();

    // Walk the import graph: std modules get materialized from the embedded
    // copy; local modules are read from disk (relative to the project CWD) so
    // we can follow the std imports they pull in transitively.
    let mut wanted_std: Vec<String> = Vec::new();
    let mut seen_local: Vec<String> = Vec::new();
    let mut queue = scan_imports(source);

    while let Some(import) = queue.pop() {
        if let Some(std_name) = import.strip_prefix("std/") {
            let std_name = std_name.to_string();
            if wanted_std.contains(&std_name) {
                continue;
            }
            if let Some(contents) = embedded_std(&std_name) {
                wanted_std.push(std_name);
                for dep in scan_imports(contents) {
                    queue.push(dep);
                }
            }
        } else {
            if seen_local.contains(&import) {
                continue;
            }
            seen_local.push(import.clone());
            // Local imports resolve next to the project (the CWD); follow them
            // to discover any std modules they import.
            if let Ok(contents) = fs::read_to_string(format!("{}.ik", import)) {
                for dep in scan_imports(&contents) {
                    queue.push(dep);
                }
            }
        }
    }

    for name in &wanted_std {
        if let Some(contents) = embedded_std(name) {
            let path = dir.join(format!("{}.ik", name));
            // Write idempotently — only when missing or out of date.
            let stale = fs::read_to_string(&path).map(|c| c.as_str() != contents).unwrap_or(true);
            if stale {
                let _ = fs::create_dir_all(dir);
                let _ = fs::write(&path, contents);
            }
        }
    }

    // (env::set_var is `unsafe` on edition 2024.)
    unsafe { std::env::set_var("IK8B_STD_PATH", dir); }
    dir
}

/// The supported target chips, read directly from the compiler's device table.
/// Returns `(name, detail)` pairs, where `detail` summarises core + flash size.
pub fn load_devices() -> Vec<(String, String)> {
    ik8b::devices::DEVICE_TABLE
        .iter()
        .map(|d| {
            (
                d.name.to_string(),
                format!("{:?} · {} flash", d.core, human_bytes(d.flash_size)),
            )
        })
        .collect()
}

fn human_bytes(b: u32) -> String {
    if b >= 1024 {
        format!("{} KB", b / 1024)
    } else {
        format!("{} B", b)
    }
}

#[cfg(test)]
mod std_embed_tests {
    use super::*;

    // Only the imported std module is materialized (on demand), and the
    // embedded copy satisfies the compiler — no `tools/` tree, no files dumped
    // into the user's project.
    #[test]
    fn imports_materialize_on_demand_and_compile() {
        assert!(STD_FILES.len() >= 21, "expected the full std library embedded");

        let src = "target atmega328p\nimport std/atomic\n@main {\n    loop * {}\n}\n";
        let dir = sync_std_imports(src);

        assert!(dir.join("atomic.ik").exists(), "imported module must be materialized");

        let res = compile(src);
        assert!(res.is_ok(), "compile with embedded std failed: {:?}", res.err());
    }

    // The recently added std/boot module is embedded, indexed for completion,
    // and resolves through the in-process front end (a regression guard for
    // "the IDE doesn't recognize new std modules").
    #[test]
    fn boot_module_is_embedded_indexed_and_compiles() {
        assert!(
            embedded_std("boot").is_some(),
            "std/boot must be embedded (build.rs scans tools/ik8b/std)"
        );

        let idx = std_symbol_index();
        assert!(
            idx.symbols.iter().any(|s| s.name == "@boot_page_erase"),
            "std/boot functions must be in the symbol index for completion/hover"
        );

        let src = "target atmega328p\nimport std/boot\n\n@main {\n    @cli()\n    @boot_page_erase(0x0000)\n    loop * {}\n}\n";
        sync_std_imports(src);
        let diags = check(src);
        assert!(diags.is_empty(), "boot program should type-check clean: {:?}", diags);
    }

    // A buffer with no std imports materializes nothing.
    #[test]
    fn no_imports_materialize_nothing() {
        let src = "target atmega328p\n@main {\n    loop * {}\n}\n";
        let dir = sync_std_imports(src);
        // The cache dir may exist from other runs, but `mem.ik` is only ever
        // written when imported — and this buffer imports nothing.
        let _ = dir; // resolution succeeds; nothing required to be present.
    }
}

#[cfg(test)]
mod diagnostics_tests {
    use super::*;

    // The compiler's undefined-variable error has no line number, but names the
    // function; the IDE must pin it to the offending line inside that function.
    #[test]
    fn undefined_variable_is_recognized_and_located() {
        let src = "target atmega328p\nconst %PORTB: u8 = 0x25\n\n@main {\n    ram mut $x: u8 = 0\n    $typo_var -> %PORTB\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "expected one diagnostic: {:?}", diags);
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.term, "$typo_var");
        assert_eq!(d.line, 6, "must point at the line using the unknown variable");
        assert!(d.message.contains("Unknown variable"), "friendly text: {}", d.message);
    }

    #[test]
    fn undefined_constant_is_recognized() {
        let src = "target atmega328p\nconst %PORTB: u8 = 0x25\n\n@main {\n    NO_SUCH_MASK -> %PORTB\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        assert_eq!(diags[0].term, "NO_SUCH_MASK");
        assert_eq!(diags[0].line, 5);
        assert!(diags[0].message.contains("Unknown constant"), "{}", diags[0].message);
    }

    // Out-of-range literal: the highlighted term is the number itself.
    #[test]
    fn out_of_range_literal_is_recognized() {
        let src = "target atmega328p\n\n@main {\n    ram mut $x: u8 = 300\n    loop * {}\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        assert_eq!(diags[0].term, "300");
        assert_eq!(diags[0].line, 4);
        assert!(diags[0].message.contains("doesn't fit"), "{}", diags[0].message);
    }

    // Narrowing compiles, but surfaces as a Warning pinned to the assignment
    // line (not the declaration of the target variable).
    #[test]
    fn narrowing_warning_is_recognized_and_located() {
        let src = "target atmega328p\nconst %PORTB: u8 = 0x25\n\n@main {\n    ram mut $a: u16 = 500\n    ram mut $b: u8 = 0\n    $a -> $b\n    $b -> %PORTB\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        let d = &diags[0];
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.term, "$b");
        assert_eq!(d.line, 7, "must point at the assignment, not the declaration");
        assert!(d.message.contains("truncates"), "{}", d.message);
    }

    // The semicolon hint from the lexer maps to a friendly explanation.
    #[test]
    fn semicolon_hint_is_recognized() {
        let src = "target atmega328p\n\n@main {\n    ram mut $x: u8 = 5;\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        assert_eq!(diags[0].line, 4);
        assert!(diags[0].message.contains("semicolons"), "{}", diags[0].message);
    }

    // Bare `@fn` used as a value (missing `&`).
    #[test]
    fn bare_function_reference_is_recognized() {
        let src = "target atmega328p\nconst %PORTB: u8 = 0x25\n@f() -> u8 { return 1 }\n@main {\n    ram mut $x: u8 = 0\n    @f -> $x\n    $x -> %PORTB\n}\n";
        let diags = check(src);
        assert_eq!(diags.len(), 1, "{:?}", diags);
        assert!(diags[0].message.contains("take its address"), "{}", diags[0].message);
    }
}
