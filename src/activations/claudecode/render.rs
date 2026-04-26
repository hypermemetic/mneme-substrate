/// Rendering arbor trees back to Claude session JSONL format
///
/// This module handles converting arbor node structures (including collapsed view nodes)
/// back into the Claude session JSONL format.

use crate::activations::arbor::{ArborStorage, NodeId, NodeType, TreeId, CollapseType};
use crate::activations::claudecode::sessions::SessionEvent;
use super::types::NodeEvent;
use serde_json::Value;

// ═══════════════════════════════════════════════════════════════════════════
// RENDERING STRATEGIES
// ═══════════════════════════════════════════════════════════════════════════

/// Controls how to resolve range references during rendering
#[derive(Debug, Clone)]
pub enum RenderMode {
    /// Fully expand all range references (may be large)
    FullExpansion,

    /// Show collapsed ranges as placeholder text
    Placeholders,

    /// Expand up to a certain depth
    PartialExpansion { max_depth: usize },
}

// ═══════════════════════════════════════════════════════════════════════════
// NODE RENDERING
// ═══════════════════════════════════════════════════════════════════════════

/// Render a single arbor node to NodeEvent, resolving range references if needed
pub async fn render_node(
    arbor: &ArborStorage,
    tree_id: &TreeId,
    node_id: &NodeId,
    mode: &RenderMode,
) -> Result<Vec<NodeEvent>, String> {
    let tree = arbor.tree_get(tree_id).await.map_err(|e| e.to_string())?;
    let node = tree.nodes.get(node_id)
        .ok_or_else(|| format!("Node not found: {}", node_id))?;

    match &node.data {
        NodeType::Text { content } => {
            // Check if this is a range reference (empty content + metadata)
            if content.is_empty() && node.metadata.is_some() {
                if let Some(range_handle) = extract_range_handle(&node.metadata) {
                    return render_range_reference(arbor, &range_handle, mode).await;
                }
            }

            // Regular node - parse NodeEvent
            let node_event: NodeEvent = serde_json::from_str(content)
                .map_err(|e| format!("Failed to parse NodeEvent: {}", e))?;

            Ok(vec![node_event])
        }
        NodeType::External { .. } => {
            // External nodes not supported in session export
            Ok(vec![])
        }
    }
}

/// Extract range handle from node metadata
fn extract_range_handle(metadata: &Option<Value>) -> Option<RangeHandleRef> {
    let meta = metadata.as_ref()?;
    let handle = meta.get("range_handle")?;

    Some(RangeHandleRef {
        tree_id: handle.get("tree_id")?.as_str()?.to_string(),
        start_node: handle.get("start_node")?.as_str()?.to_string(),
        end_node: handle.get("end_node")?.as_str()?.to_string(),
        collapse_type: handle.get("collapse_type")?.as_str()?.to_string(),
    })
}

#[derive(Debug, Clone)]
struct RangeHandleRef {
    tree_id: String,
    start_node: String,
    end_node: String,
    collapse_type: String,
}

/// Render a range reference based on the render mode
async fn render_range_reference(
    arbor: &ArborStorage,
    range: &RangeHandleRef,
    mode: &RenderMode,
) -> Result<Vec<NodeEvent>, String> {
    match mode {
        RenderMode::FullExpansion => {
            // Get all nodes in range and render them
            let tree_id = range.tree_id.parse()
                .map_err(|e| format!("Invalid tree_id: {}", e))?;
            let start = range.start_node.parse()
                .map_err(|e| format!("Invalid start_node: {}", e))?;
            let end = range.end_node.parse()
                .map_err(|e| format!("Invalid end_node: {}", e))?;

            // Get range content
            let content = arbor.range_get(
                &tree_id,
                &start,
                &end,
                &CollapseType::TextMerge
            ).await.map_err(|e| e.to_string())?;

            // For text merge, return the merged content as events
            match content {
                crate::activations::arbor::RangeContent::Text { node_ids, .. } => {
                    let mut events = Vec::new();
                    for node_id in node_ids {
                        let node_events = render_node(
                            arbor,
                            &tree_id,
                            &node_id,
                            &RenderMode::FullExpansion
                        ).await?;
                        events.extend(node_events);
                    }
                    Ok(events)
                }
                _ => Ok(vec![])
            }
        }
        RenderMode::Placeholders => {
            // Return a placeholder text node
            Ok(vec![NodeEvent::ContentText {
                text: format!("[Collapsed: {} nodes]",
                    // Would need to fetch actual count
                    "N"
                )
            }])
        }
        RenderMode::PartialExpansion { max_depth } => {
            if *max_depth > 0 {
                // Recursively expand with reduced depth
                let mut mode_reduced = RenderMode::PartialExpansion {
                    max_depth: max_depth - 1
                };
                render_range_reference(arbor, range, &mode_reduced).await
            } else {
                // Hit depth limit, use placeholder
                render_range_reference(arbor, range, &RenderMode::Placeholders).await
            }
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// TREE RENDERING
// ═══════════════════════════════════════════════════════════════════════════

/// Render an entire arbor tree to SessionEvents (for JSONL export)
pub async fn render_tree_to_session_events(
    arbor: &ArborStorage,
    tree_id: &TreeId,
    mode: RenderMode,
) -> Result<Vec<SessionEvent>, String> {
    let tree = arbor.tree_get(tree_id).await.map_err(|e| e.to_string())?;

    // Traverse tree in DFS order
    let node_ids = traverse_tree_dfs(&tree);

    let mut session_events = Vec::new();
    let mut current_message: Option<MessageBuilder> = None;

    for node_id in node_ids {
        let node_events = render_node(arbor, tree_id, &node_id, &mode).await?;

        for event in node_events {
            match event {
                NodeEvent::UserMessage { content } => {
                    // Complete previous message if any
                    if let Some(builder) = current_message.take() {
                        session_events.push(builder.build());
                    }

                    // Create user event
                    session_events.push(SessionEvent::User {
                        data: create_user_event(content)
                    });
                }

                NodeEvent::AssistantStart => {
                    // Start new assistant message
                    current_message = Some(MessageBuilder::new_assistant());
                }

                NodeEvent::ContentText { text } => {
                    if let Some(builder) = &mut current_message {
                        builder.add_text(text);
                    }
                }

                NodeEvent::ContentToolUse { id, name, input } => {
                    if let Some(builder) = &mut current_message {
                        builder.add_tool_use(id, name, input);
                    }
                }

                NodeEvent::ContentThinking { thinking } => {
                    if let Some(builder) = &mut current_message {
                        builder.add_thinking(thinking);
                    }
                }

                NodeEvent::AssistantComplete { .. } => {
                    // Complete assistant message
                    if let Some(builder) = current_message.take() {
                        session_events.push(builder.build());
                    }
                }

                _ => {
                    // Other event types
                }
            }
        }
    }

    // Complete any pending message
    if let Some(builder) = current_message.take() {
        session_events.push(builder.build());
    }

    Ok(session_events)
}

// ═══════════════════════════════════════════════════════════════════════════
// HELPER TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Builds SessionEvent::Assistant from individual content blocks
struct MessageBuilder {
    role: String,
    content_blocks: Vec<Value>,
}

impl MessageBuilder {
    fn new_assistant() -> Self {
        Self {
            role: "assistant".to_string(),
            content_blocks: Vec::new(),
        }
    }

    fn add_text(&mut self, text: String) {
        self.content_blocks.push(serde_json::json!({
            "type": "text",
            "text": text
        }));
    }

    fn add_tool_use(&mut self, id: String, name: String, input: Value) {
        self.content_blocks.push(serde_json::json!({
            "type": "tool_use",
            "id": id,
            "name": name,
            "input": input
        }));
    }

    fn add_thinking(&mut self, thinking: String) {
        self.content_blocks.push(serde_json::json!({
            "type": "thinking",
            "thinking": thinking
        }));
    }

    fn build(self) -> SessionEvent {
        // This is simplified - would need full AssistantEvent structure
        use crate::activations::claudecode::sessions::AssistantEvent;
        use crate::activations::claudecode::sessions::AssistantMessage;

        SessionEvent::Assistant {
            data: AssistantEvent {
                uuid: uuid::Uuid::new_v4().to_string(),
                session_id: String::new(), // Would need from context
                timestamp: chrono::Utc::now().to_rfc3339(),
                message: AssistantMessage::Simple(
                    serde_json::to_string(&self.content_blocks).unwrap()
                ),
                usage: None,
            }
        }
    }
}

fn create_user_event(content: String) -> crate::activations::claudecode::sessions::UserEvent {
    use crate::activations::claudecode::sessions::{UserEvent, UserMessage};

    UserEvent {
        uuid: uuid::Uuid::new_v4().to_string(),
        parent_uuid: None,
        session_id: String::new(),
        timestamp: chrono::Utc::now().to_rfc3339(),
        cwd: String::new(),
        message: UserMessage { content },
    }
}

// Placeholder - would import from arbor/views.rs
fn traverse_tree_dfs(tree: &crate::activations::arbor::Tree) -> Vec<NodeId> {
    // Would use the DFS implementation from views.rs
    vec![]
}
