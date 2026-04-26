use mneme_substrate::activations::arbor::{ArborConfig, ArborStorage, CollapseType};
use tempfile::TempDir;

#[tokio::test]
async fn test_view_collapse_text_runs() {
    // Create temporary storage
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test.db");
    let config = ArborConfig {
        db_path,
        ..Default::default()
    };
    let storage = ArborStorage::new(config).await.unwrap();

    println!("\n=== Testing View Collapse System ===\n");

    // 1. Create test tree
    println!("1. Creating test tree...");
    let tree_id = storage.tree_create(None, "test-user").await.unwrap();
    println!("✓ Created tree: {tree_id}\n");

    // 2. Add consecutive text nodes (simulating streaming response)
    println!("2. Adding consecutive text nodes...");
    let texts = vec![
        "The quick ",
        "brown fox ",
        "jumps over ",
        "the lazy ",
        "dog. ",
        "This is ",
        "a test ",
        "of the ",
        "text run ",
        "detection ",
        "system.",
    ];

    // Get the tree's root to use as parent for first node
    let tree = storage.tree_get(&tree_id).await.unwrap();
    let mut parent_id: Option<mneme_substrate::activations::arbor::NodeId> = Some(tree.root);

    for text in &texts {
        let node_id = storage
            .node_create_text(&tree_id, parent_id, text.to_string(), None)
            .await
            .unwrap();
        println!("  Added: \"{}\" ({} chars)", text.trim(), text.len());
        parent_id = Some(node_id);
    }

    // Debug: Check tree structure
    println!("\n2.5. Checking tree structure...");
    let tree = storage.tree_get(&tree_id).await.unwrap();
    println!("  Tree has {} nodes total", tree.nodes.len());
    println!("  Root node: {}", tree.root);

    // 3. Detect text runs
    println!("\n3. Detecting text runs (min_length=3)...");
    let runs = storage.view_detect_text_runs(&tree_id, 3).await.unwrap();
    println!("✓ Found {} text run(s):", runs.len());
    for run in &runs {
        println!("  - Nodes: {}, Chars: {}", run.length, run.char_count);
        println!("    Start: {}...", &run.start_node.to_string()[..8]);
        println!("    End: {}...", &run.end_node.to_string()[..8]);
    }

    if runs.is_empty() {
        println!("\nDEBUG: No runs found. Let's inspect the tree:");
        for (i, (node_id, node)) in tree.nodes.iter().enumerate() {
            println!("  Node {}: {} parent={:?}",
                i,
                node_id.to_string().chars().take(8).collect::<String>(),
                node.parent.as_ref().map(|p| p.to_string().chars().take(8).collect::<String>())
            );
        }
    }

    assert!(!runs.is_empty(), "Should detect at least one text run");
    assert_eq!(runs[0].length, 11, "Should detect run of all 11 nodes");

    // 4. Create collapsed view
    println!("\n4. Creating collapsed view tree...");
    let (view_tree_id, collapsed_runs) = storage
        .view_collapse_text_runs(&tree_id, 3, "test-user")
        .await
        .unwrap();
    println!("✓ Created view tree: {view_tree_id}");
    println!("✓ Collapsed {} run(s)", collapsed_runs.len());

    // 5. Get view tree structure
    println!("\n5. View tree structure:");
    let view_tree = storage.tree_get(&view_tree_id).await.unwrap();
    println!("  Root: {}", view_tree.root);
    println!("  Total nodes in view: {}", view_tree.nodes.len());
    println!("  Original nodes: {}", texts.len());
    println!("  Compression: {} nodes → {} nodes", texts.len(), view_tree.nodes.len());

    assert!(
        view_tree.nodes.len() < texts.len(),
        "View should have fewer nodes than original"
    );

    // 6. Test range_get to retrieve merged content
    println!("\n6. Testing range_get to retrieve merged content...");
    let run = &runs[0];
    let range_content = storage
        .range_get(&tree_id, &run.start_node, &run.end_node, &CollapseType::TextMerge)
        .await
        .unwrap();

    match range_content {
        mneme_substrate::activations::arbor::RangeContent::Text {
            content,
            node_count,
            ..
        } => {
            println!("✓ Merged {node_count} nodes into:");
            println!("  \"{content}\"");
            assert_eq!(node_count, texts.len(), "Should merge all text nodes");

            let expected: String = texts.iter().copied().collect();
            assert_eq!(content, expected, "Merged content should match original");
        }
        _ => panic!("Expected Text content"),
    }

    println!("\n✓ All tests passed!");
}
