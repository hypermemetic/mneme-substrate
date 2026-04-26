# ARBOR-EVENTS-1: Store Each Claude Event as Arbor Node

**Status**: In Progress
**Epoch**: CLAUDECODE

---

## Problem

Currently only user and assistant messages are stored as arbor nodes. All intermediate events (tool uses, tool results, thinking, content chunks) are ephemeral stream events that disappear after the chat completes. This makes it impossible to reconstruct the full conversation flow from arbor alone.

## Solution

Store each Claude event as an arbor text node with inline JSON content.

### Node Structure

```
root (empty text)
└─ user message (external node → claudecode_messages)
   ├─ content_0 (text node, JSON: {type: "content", text: "..."})
   ├─ thinking_0 (text node, JSON: {type: "thinking", thinking: "..."})
   ├─ tool_use_0 (text node, JSON: {type: "tool_use", name: "Write", id: "toolu_123", input: {...}})
   ├─ tool_result_0 (text node, JSON: {type: "tool_result", tool_use_id: "toolu_123", output: "...", is_error: false})
   ├─ tool_use_1 (text node, JSON: {type: "tool_use", name: "Bash", ...})
   ├─ tool_result_1 (text node, JSON: {type: "tool_result", ...})
   └─ assistant message (external node → claudecode_messages)
```

### Event Types Stored

1. **Content blocks**: Each `ChatEvent::Content` → text node with `{type: "content", text: "..."}`
2. **Thinking blocks**: Each `ChatEvent::Thinking` → text node with `{type: "thinking", thinking: "..."}`
3. **Tool uses**: Each `ChatEvent::ToolUse` → text node with `{type: "tool_use", name, id, input}`
4. **Tool results**: Each `ChatEvent::ToolResult` → text node with `{type: "tool_result", tool_use_id, output, is_error}`
5. **User/assistant messages**: Keep as external nodes (current behavior)

## Implementation

### Files to Modify

1. **`activation.rs`**: Add arbor node creation for each event type
   - In `chat()` streaming loop (lines 304-430)
   - In `chat_async()` background task (lines 775-1118)

2. **`types.rs`**: Add helper structs for event JSON serialization (optional, could use inline `serde_json::json!`)

### Code Changes

Track current parent and create nodes:

```rust
// After creating user_node_id (line 263)
let mut current_parent = user_node_id;
let mut event_counter = 0;

// In event loop, for each ChatEvent before yielding:
match &event {
    ChatEvent::Content { text } => {
        let content_json = serde_json::json!({
            "type": "content",
            "text": text,
        });
        if let Ok(node_id) = storage.arbor().node_create_text(
            &config.head.tree_id,
            Some(current_parent),
            content_json.to_string(),
            None,
        ).await {
            current_parent = node_id;
            event_counter += 1;
        }
        yield event;
    }
    ChatEvent::ToolUse { tool_name, tool_use_id, input } => {
        let tool_json = serde_json::json!({
            "type": "tool_use",
            "name": tool_name,
            "id": tool_use_id,
            "input": input,
        });
        if let Ok(node_id) = storage.arbor().node_create_text(
            &config.head.tree_id,
            Some(current_parent),
            tool_json.to_string(),
            None,
        ).await {
            current_parent = node_id;
        }
        yield event;
    }
    // Similar for ToolResult, Thinking
    _ => yield event,
}
```

### Arbor Query Method

Add to claudecode activation:

```rust
#[plexus_method]
pub async fn get_tree(
    &self,
    ctx: PlexusContext,
    name: String,
) -> PlexusStream<GetTreeResult> {
    // Get session config
    let config = storage.session_get_by_name(&name).await?;

    // Return tree_id and head
    Ok(GetTreeResult {
        tree_id: config.head.tree_id,
        head: config.head.node_id,
    })
}
```

## Open Questions

1. **Should content chunks be individual nodes or accumulated?**
   - Individual: precise, but many nodes for long responses
   - Accumulated: create one node per content block (need to detect block boundaries)
   - **Decision**: Start with individual, optimize later if needed

2. **Parent linkage**: Should events be siblings (all children of user) or sequential (each child of previous)?
   - Siblings: flat structure, easier to query
   - Sequential: linear chain, shows order explicitly
   - **Decision**: Sequential chain to preserve strict ordering

3. **Ephemeral sessions**: Should ephemeral events get ephemeral nodes?
   - **Decision**: Yes, use `node_create_text_ephemeral` (need to add this to arbor if missing)

## Testing

1. Run orcha with a task that uses tools
2. Query arbor tree after completion
3. Verify all tool uses, results, and content appear as nodes
4. Check that tree structure is correct (user → events → assistant)

