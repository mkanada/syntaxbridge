//! Baseline timing for the four `libclang` extraction passes ingestion runs
//! in sequence (`type_catalog`, `source_catalog`, `function_catalog`,
//! `pointer_catalog`) — see the "Oportunidades priorizadas" performance
//! review that flagged the lack of any timing regression coverage (the only
//! existing measurements were `#[ignore]`d diagnosis tests printing via
//! `eprintln!`, with no saved baseline). Run with `cargo bench -p
//! syntax-bridge-server --bench extraction`.
//!
//! The fixture is a synthetic CMake project (one `Base` class, a
//! configurable number of `Widget` subclasses with overloaded methods and a
//! raw-pointer member) generated fresh for each run rather than checked in,
//! so the corpus size can be tuned here without touching a golden file.
//! Building it (writing sources, running `cmake`) happens once per
//! `cargo bench` invocation, outside every timed sample.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use criterion::{Criterion, criterion_group, criterion_main};
use syntax_bridge_server::ingest::{self, CreateProjectRequest, CreatedProject};
use syntax_bridge_server::{
    extraction, function_catalog, pointer_catalog, source_catalog, type_catalog,
};

const WIDGET_COUNT: usize = 12;

fn bench_extraction(c: &mut Criterion) {
    let workspace = TempWorkspace::new("extraction-bench").expect("create temp workspace");
    let source_dir = workspace.path().join("fixture");
    write_extraction_fixture(&source_dir).expect("write fixture sources");

    let project = ingest_fixture(&source_dir, workspace.path());
    let units = project.compilation_units;
    let project_root = project.input_source_dir;

    let mut group = c.benchmark_group("extraction");
    // Each pass runs a `libclang` parse per compilation unit under the
    // hood, so the default 100 samples would take a while; 20 is enough to
    // catch a meaningful regression without turning `cargo bench` into a
    // multi-minute run.
    group.sample_size(20);

    group.bench_function("type_catalog", |b| {
        b.iter(|| {
            type_catalog::extract_type_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract type catalog")
        })
    });

    group.bench_function("source_catalog", |b| {
        b.iter(|| {
            source_catalog::extract_source_files(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract source files")
        })
    });

    group.bench_function("function_catalog", |b| {
        b.iter(|| {
            function_catalog::extract_function_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract function catalog")
        })
    });

    group.bench_function("pointer_catalog", |b| {
        b.iter(|| {
            pointer_catalog::extract_pointer_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract pointer catalog")
        })
    });

    // The two benchmarks actually comparable for item #1 ("unify the four
    // libclang passes"): `create_project`'s pipeline before that change ran
    // the four passes above sequentially (four full `libclang` parses per
    // compilation unit); `extraction::extract_project_catalogs_cancellable`
    // is what it calls now (two parses per compilation unit, one per
    // body-visibility requirement). The four benchmarks above stay in place
    // because they measure each standalone `extract_*` entry point, which
    // still does its own independent parse and is unaffected by that change
    // — this pair is the one that shows its effect.
    group.bench_function("four_sequential_passes_before", |b| {
        b.iter(|| {
            let type_catalog = type_catalog::extract_type_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract type catalog");
            let source_files = source_catalog::extract_source_files(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract source files");
            let function_catalog = function_catalog::extract_function_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract function catalog");
            let pointer_catalog = pointer_catalog::extract_pointer_catalog(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
            )
            .expect("extract pointer catalog");
            (
                type_catalog,
                source_files,
                function_catalog,
                pointer_catalog,
            )
        })
    });

    group.bench_function("unified_two_passes_after", |b| {
        b.iter(|| {
            extraction::extract_project_catalogs_cancellable(
                criterion::black_box(&units),
                criterion::black_box(&project_root),
                None,
                None,
                None,
                None,
                None,
            )
            .expect("extract project catalogs")
        })
    });

    group.finish();
}

criterion_group!(benches, bench_extraction);
criterion_main!(benches);

fn widget_header(index: usize) -> String {
    format!(
        r#"#pragma once
#include "base.h"

class Widget{index} : public Base {{
public:
    Widget{index}();
    ~Widget{index}() override;

    int process(int value);
    double process(double value);
    void attach(Base* other);

private:
    Base* delegate_;
    int state_;
}};
"#
    )
}

fn widget_source(index: usize) -> String {
    format!(
        r#"#include "widget{index}.h"

Widget{index}::Widget{index}() : delegate_(nullptr), state_(0) {{}}

Widget{index}::~Widget{index}() {{}}

int Widget{index}::process(int value) {{
    state_ += value;
    if (delegate_ != nullptr) {{
        state_ += delegate_->touch();
    }}
    return state_;
}}

double Widget{index}::process(double value) {{
    return static_cast<double>(state_) + value;
}}

void Widget{index}::attach(Base* other) {{
    delegate_ = other;
}}
"#
    )
}

fn write_extraction_fixture(source_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(source_dir)?;

    fs::write(
        source_dir.join("base.h"),
        r#"#pragma once

class Base {
public:
    virtual ~Base() = default;
    virtual int touch() { return 1; }
};
"#,
    )?;

    let mut sources = Vec::new();
    for index in 0..WIDGET_COUNT {
        fs::write(
            source_dir.join(format!("widget{index}.h")),
            widget_header(index),
        )?;
        let source_name = format!("widget{index}.cpp");
        fs::write(source_dir.join(&source_name), widget_source(index))?;
        sources.push(source_name);
    }

    let mut main_source = String::from("#include \"base.h\"\n");
    for index in 0..WIDGET_COUNT {
        main_source.push_str(&format!("#include \"widget{index}.h\"\n"));
    }
    main_source.push_str("\nint main() {\n    Base base;\n");
    for index in 0..WIDGET_COUNT {
        main_source.push_str(&format!(
            "    Widget{index} widget{index};\n    widget{index}.attach(&base);\n    widget{index}.process(1);\n    widget{index}.process(1.0);\n"
        ));
    }
    main_source.push_str("    return 0;\n}\n");
    fs::write(source_dir.join("main.cpp"), main_source)?;
    sources.push("main.cpp".to_owned());

    let sources_list = sources.join(" ");
    fs::write(
        source_dir.join("CMakeLists.txt"),
        format!(
            r#"cmake_minimum_required(VERSION 3.16)
project(syntax_bridge_extraction_bench LANGUAGES CXX)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
add_executable(syntax_bridge_extraction_bench {sources_list})
"#
        ),
    )?;

    Ok(())
}

fn ingest_fixture(source_dir: &Path, workspace: &Path) -> CreatedProject {
    let archive_path = workspace.join("fixture.tar.gz");
    let output = Command::new("tar")
        .arg("-czf")
        .arg(&archive_path)
        .arg("-C")
        .arg(source_dir.parent().expect("fixture dir has a parent"))
        .arg(source_dir.file_name().expect("fixture dir has a name"))
        .output()
        .expect("run tar");
    assert_success(output);

    ingest::create_project(CreateProjectRequest {
        name: "bench".to_owned(),
        workspace_dir: workspace.join("projects"),
        archive_path,
    })
    .expect("ingest extraction bench fixture")
}

fn assert_success(output: Output) {
    assert!(
        output.status.success(),
        "command failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

struct TempWorkspace {
    path: PathBuf,
}

impl TempWorkspace {
    fn new(name: &str) -> io::Result<Self> {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "syntax-bridge-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path)?;

        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
