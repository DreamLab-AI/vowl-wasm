//! Phase 3 Performance Benchmarks
//!
//! Validates Phase 3 performance targets:
//! - tick() time: <10ms (target: 8ms)
//! - getGraphData() serialization: <5ms
//! - checkNodeClick(): <1ms for 1,000 nodes
//! - Bundle size: <1.5MB WASM

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use nalgebra::Vector3;
use std::time::Duration;
use vowl_wasm::interaction::{find_closest_node_hit, NodeHitTest, Ray};
use vowl_wasm::{
    bindings::WebVowl,
    graph::builder::GraphBuilder,
    layout::{simulation::ForceSimulation, LayoutAlgorithm},
    ontology::{
        ClassAttributes, ClassNode, OntologyData, OntologyMetadata, Property,
        PropertyCharacteristics, PropertyType,
    },
};

fn create_test_ontology(num_classes: usize, num_properties: usize) -> OntologyData {
    let mut classes = Vec::new();
    for i in 0..num_classes {
        classes.push(ClassNode {
            id: format!("class{}", i),
            iri: format!("http://test.org/Class{}", i),
            label: format!("Class {}", i),
            class_type: "owl:Class".to_string(),
            equivalent: vec![],
            attributes: ClassAttributes::default(),
            ontology_meta: None,
        });
    }

    let mut properties = Vec::new();
    for i in 0..num_properties {
        let domain_idx = i % num_classes;
        let range_idx = (i + 1) % num_classes;

        properties.push(Property {
            id: format!("prop{}", i),
            iri: format!("http://test.org/prop{}", i),
            label: format!("Property {}", i),
            property_type: PropertyType::ObjectProperty,
            domain: format!("class{}", domain_idx),
            range: format!("class{}", range_idx),
            characteristics: PropertyCharacteristics::default(),
            inverse_of: None,
            equivalent_to: vec![],
            disjoint_with: vec![],
        });
    }

    OntologyData {
        metadata: OntologyMetadata {
            iri: "http://test.org/bench".to_string(),
            version: Some("1.0.0".to_string()),
            title: Some("Phase 3 Benchmark Ontology".to_string()),
            description: Some("Ontology for Phase 3 performance benchmarks".to_string()),
        },
        classes,
        properties,
        namespaces: vec![],
    }
}

/// Benchmark tick() performance with metadata
/// Target: <10ms (ideal: 8ms)
fn bench_tick_with_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_tick");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    // Test with realistic graph sizes
    for size in [50, 100, 200, 500].iter() {
        let ontology = create_test_ontology(*size, (*size * 3) / 4);
        let mut graph = GraphBuilder::from_ontology(&ontology).unwrap();
        let mut sim = ForceSimulation::new();
        sim.initialize(&mut graph).unwrap();

        group.bench_with_input(BenchmarkId::new("tick_metadata", size), size, |b, _| {
            b.iter(|| {
                let mut test_graph = graph.clone();
                let mut test_sim = ForceSimulation::new();
                test_sim.initialize(&mut test_graph).unwrap();
                test_sim.tick(black_box(&mut test_graph)).unwrap();
                black_box(test_graph);
            });
        });
    }

    group.finish();
}

/// Benchmark getGraphData() serialization
/// Target: <5ms
fn bench_get_graph_data(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_serialization");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(100);

    for size in [50, 100, 200, 500].iter() {
        let ontology = create_test_ontology(*size, (*size * 3) / 4);
        let graph = GraphBuilder::from_ontology(&ontology).unwrap();

        group.bench_with_input(BenchmarkId::new("getGraphData", size), size, |b, _| {
            b.iter(|| {
                let json_str = serde_json::to_string(&graph.nodes()).unwrap();
                black_box(json_str);
            });
        });
    }

    group.finish();
}

/// Benchmark checkNodeClick() at various scales
/// Target: <1ms for 1,000 nodes
fn bench_check_node_click(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_interaction");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(200);

    for size in [100, 500, 1000, 2000].iter() {
        let ontology = create_test_ontology(*size, (*size * 3) / 4);
        let graph = GraphBuilder::from_ontology(&ontology).unwrap();

        // Ray straight down -Z through the layout centre, matching how a click
        // is unprojected from a camera at z = 10.
        let nodes: Vec<NodeHitTest> = graph
            .nodes()
            .iter()
            .map(|node| NodeHitTest {
                id: node.id.clone(),
                position: Vector3::new(node.visual.x as f32, node.visual.y as f32, 0.0),
                radius: 20.0,
            })
            .collect();
        let ray = Ray::new(Vector3::new(0.0, 0.0, 10.0), Vector3::new(0.0, 0.0, -1.0));

        group.bench_with_input(BenchmarkId::new("checkNodeClick", size), size, |b, _| {
            b.iter(|| {
                black_box(find_closest_node_hit(black_box(&ray), black_box(&nodes)));
            });
        });
    }

    group.finish();
}

/// Benchmark metadata access performance
fn bench_metadata_access(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_metadata");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(200);

    let ontology = create_test_ontology(500, 375);
    let graph = GraphBuilder::from_ontology(&ontology).unwrap();

    group.bench_function("access_all_metadata", |b| {
        b.iter(|| {
            let metadata = graph.metadata();
            black_box((
                metadata.class_count,
                metadata.property_count,
                metadata.max_degree,
                metadata.density,
            ));
        });
    });

    group.bench_function("access_node_metadata", |b| {
        b.iter(|| {
            for node in graph.nodes().iter().take(10) {
                black_box((&node.id, &node.label, &node.semantic.iri, &node.node_type));
            }
        });
    });

    group.finish();
}

/// Benchmark full simulation workflow with metadata
fn bench_full_workflow(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_workflow");
    group.measurement_time(Duration::from_secs(15));
    group.sample_size(50);

    let ontology = create_test_ontology(100, 75);

    group.bench_function("load_simulate_serialize", |b| {
        b.iter(|| {
            // Load ontology
            let mut graph = GraphBuilder::from_ontology(black_box(&ontology)).unwrap();

            // Initialize simulation
            let mut sim = ForceSimulation::new();
            sim.initialize(&mut graph).unwrap();

            // Run 10 ticks
            for _ in 0..10 {
                sim.tick(&mut graph).unwrap();
            }

            // Serialize graph data
            let json = serde_json::to_string(&graph.nodes()).unwrap();

            // Access metadata
            let metadata = graph.metadata();

            black_box((json, metadata));
        });
    });

    group.finish();
}

/// Benchmark WASM API surface
fn bench_wasm_api(c: &mut Criterion) {
    let mut group = c.benchmark_group("phase3_wasm_api");
    group.measurement_time(Duration::from_secs(10));
    group.sample_size(50);

    // Note: This would require WASM test environment
    // For now, benchmark the Rust implementation

    let ontology = create_test_ontology(100, 75);
    let json_ontology = serde_json::to_string(&ontology).unwrap();

    group.bench_function("wasm_load_ontology", |b| {
        b.iter(|| {
            let mut webvowl = WebVowl::new();
            webvowl.load_ontology(black_box(&json_ontology)).unwrap();
            black_box(webvowl);
        });
    });

    group.bench_function("wasm_tick_operation", |b| {
        let mut webvowl = WebVowl::new();
        webvowl.load_ontology(&json_ontology).unwrap();
        webvowl.init_simulation().unwrap();

        b.iter(|| {
            webvowl.tick().unwrap();
            black_box(&webvowl);
        });
    });

    group.finish();
}

criterion_group!(
    phase3_benches,
    bench_tick_with_metadata,
    bench_get_graph_data,
    bench_check_node_click,
    bench_metadata_access,
    bench_full_workflow,
    bench_wasm_api
);

criterion_main!(phase3_benches);
