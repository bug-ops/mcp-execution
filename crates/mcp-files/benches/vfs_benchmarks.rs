use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_files::{FileSystem, FilesBuilder};
use std::hint::black_box;

/// Benchmark `read_file` operation across different VFS sizes
fn bench_read_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("read_file");

    for size in [10, 100, 1000, 10000] {
        let vfs = create_vfs_with_files(size);
        let path = format!("/mcp-tools/servers/test/file_{}.ts", size / 2);

        group.bench_with_input(BenchmarkId::new("read", size), &path, |b, path| {
            b.iter(|| vfs.read_file(black_box(path)).unwrap());
        });
    }

    group.finish();
}

/// Benchmark exists operation for both existing and missing files
fn bench_exists(c: &mut Criterion) {
    let mut group = c.benchmark_group("exists");

    for size in [10, 100, 1000, 10000] {
        let vfs = create_vfs_with_files(size);
        let path_exist = format!("/mcp-tools/servers/test/file_{}.ts", size / 2);
        let path_missing = "/mcp-tools/servers/test/missing.ts";

        group.bench_with_input(
            BenchmarkId::new("exists_true", size),
            &path_exist,
            |b, path| b.iter(|| vfs.exists(black_box(path))),
        );

        group.bench_with_input(
            BenchmarkId::new("exists_false", size),
            &path_missing,
            |b, path| b.iter(|| vfs.exists(black_box(path))),
        );
    }

    group.finish();
}

/// Benchmark `list_dir` operation (identifies scaling issues)
fn bench_list_dir(c: &mut Criterion) {
    let mut group = c.benchmark_group("list_dir");

    for size in [10, 100, 1000] {
        let vfs = create_vfs_with_files(size);
        let path = "/mcp-tools/servers/test";

        group.bench_with_input(BenchmarkId::new("list", size), &size, |b, _| {
            b.iter(|| vfs.list_dir(black_box(path)).unwrap());
        });
    }

    group.finish();
}

/// Benchmark sequential file addition
fn bench_add_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("add_file");

    for size in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::new("sequential", size), &size, |b, &size| {
            b.iter(|| {
                let mut vfs = FileSystem::new();
                for i in 0..size {
                    vfs.add_file(
                        format!("/test_{i}.ts"),
                        format!("export const VERSION_{i} = '1.0';"),
                    )
                    .unwrap();
                }
                vfs
            });
        });
    }

    group.finish();
}

/// Benchmark `FilesBuilder` construction
fn bench_builder(c: &mut Criterion) {
    let mut group = c.benchmark_group("builder");

    for size in [10, 100, 1000] {
        let files: Vec<_> = (0..size)
            .map(|i| {
                (
                    format!("/test_{i}.ts"),
                    format!("export const VERSION_{i} = '1.0';"),
                )
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("build", size), &files, |b, files| {
            b.iter(|| {
                FilesBuilder::new()
                    .add_files(files.clone())
                    .build()
                    .unwrap()
            });
        });
    }

    group.finish();
}

/// Benchmark path validation overhead
fn bench_path_validation(c: &mut Criterion) {
    use mcp_files::FilePath;

    let paths = vec![
        ("/simple.ts", "short"),
        ("/mcp-tools/servers/test/manifest.json", "medium"),
        (
            "/very/deep/nested/path/to/some/file/that/is/really/long.ts",
            "long",
        ),
    ];

    let mut group = c.benchmark_group("path_validation");

    for (path, label) in paths {
        group.bench_with_input(BenchmarkId::new("validate", label), &path, |b, path| {
            b.iter(|| FilePath::new(black_box(path)).unwrap());
        });
    }

    group.finish();
}

/// Benchmark typical workflow: build VFS, perform multiple reads
fn bench_typical_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("workflow");

    for size in [10, 100] {
        let files: Vec<_> = (0..size)
            .map(|i| {
                (
                    format!("/mcp-tools/servers/test/file_{i}.ts"),
                    format!("export const VERSION_{i} = '1.0';"),
                )
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("build_and_read", size), &size, |b, _| {
            b.iter(|| {
                // Build VFS
                let vfs = FilesBuilder::new()
                    .add_files(files.clone())
                    .build()
                    .unwrap();

                // Perform typical operations
                for i in 0..size.min(10) {
                    let path = format!("/mcp-tools/servers/test/file_{i}.ts");
                    let _ = vfs.read_file(&path).unwrap();
                }

                let _ = vfs.list_dir("/mcp-tools/servers/test").unwrap();
            });
        });
    }

    group.finish();
}

/// Benchmark memory overhead by checking `file_count` performance
fn bench_file_count(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_count");

    for size in [10, 100, 1000, 10000] {
        let vfs = create_vfs_with_files(size);

        group.bench_with_input(BenchmarkId::new("count", size), &vfs, |b, vfs| {
            b.iter(|| vfs.file_count());
        });
    }

    group.finish();
}

/// Benchmark `all_paths` operation (sorts paths)
fn bench_all_paths(c: &mut Criterion) {
    let mut group = c.benchmark_group("all_paths");

    for size in [10, 100, 1000] {
        let vfs = create_vfs_with_files(size);

        group.bench_with_input(BenchmarkId::new("sorted", size), &vfs, |b, vfs| {
            b.iter(|| vfs.all_paths());
        });
    }

    group.finish();
}

// Helper function to create VFS with specified number of files
fn create_vfs_with_files(count: usize) -> FileSystem {
    let mut vfs = FileSystem::new();
    for i in 0..count {
        vfs.add_file(
            format!("/mcp-tools/servers/test/file_{i}.ts"),
            format!("export const VERSION_{i} = '1.0';\n// File {i}"),
        )
        .unwrap();
    }
    vfs
}

criterion_group!(
    benches,
    bench_read_file,
    bench_exists,
    bench_list_dir,
    bench_add_file,
    bench_builder,
    bench_path_validation,
    bench_typical_workflow,
    bench_file_count,
    bench_all_paths
);
criterion_main!(benches);
