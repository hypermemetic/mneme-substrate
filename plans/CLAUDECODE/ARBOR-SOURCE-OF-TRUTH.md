# Arbor as Source of Truth for Claude Code Sessions

**Status**: Planning
**Epoch**: CLAUDECODE
**Dependencies**: ARBOR-EVENTS-1

---

## Vision

Arbor becomes the **single source of truth** for all conversation state. Any subtree in arbor can be "rendered" into a valid Claude Code session. This enables:

1. **Time travel**: Start a session from any point in history
2. **Context windowing**: Render only relevant portions (last N turns, since bookmark, etc.)
3. **Lossless summarization**: Replace subtrees with summary nodes that preserve interface behavior
4. **Multi-session forking**: Create multiple concurrent sessions from different views of the same tree

---

## What a "Valid Claude Code Session Entry" Looks Like

A Claude Code session can be created in two modes:

### Mode 1: Resume (uses Claude's built-in session state)
```bash
claude --resume {claude_session_id} \
       --model sonnet \
       --system-prompt "..." \
       --permission-prompt-tool mcp__plexus__loopback_permit \
       -- "next prompt"
```

**Requirements:**
- `claude_session_id` from a previous session
- Claude maintains its own conversation history internally
- We only need to track the ID

### Mode 2: Fresh (reconstruct from message history)
```bash
# This doesn't exist in Claude CLI, but conceptually:
# Pass messages to Claude API in this format
```

```json
{
  "model": "sonnet",
  "system": "...",
  "messages": [
    {
      "role": "user",
      "content": "Write a bash script"
    },
    {
      "role": "assistant",
      "content": [
        {"type": "text", "text": "I'll help you..."},
        {"type": "tool_use", "id": "toolu_123", "name": "Write", "input": {...}}
      ]
    },
    {
      "role": "user",
      "content": [
        {"type": "tool_result", "tool_use_id": "toolu_123", "content": "..."}
      ]
    },
    {
      "role": "assistant",
      "content": "Done! The script..."
    }
  ]
}
```

**Our Goal:** Render any arbor subtree into this messages array format.

---

## Arbor Node Structure (Revised)

### Design Principle
**Every node contains complete, self-describing JSON that maps 1:1 to Claude API structures.**

### Node Types

#### 1. User Message Node
```json
{
  "type": "user_message",
  "role": "user",
  "content": "Write a bash script that prints hello world"
}
```

#### 2. Assistant Turn Start Node
```json
{
  "type": "assistant_start",
  "role": "assistant"
}
```

#### 3. Content Block Node (child of assistant_start)
```json
{
  "type": "content_text",
  "text": "I'll help you create that script..."
}
```

#### 4. Tool Use Node (child of assistant_start)
```json
{
  "type": "content_tool_use",
  "id": "toolu_01ABC123",
  "name": "Write",
  "input": {
    "file_path": "/tmp/hello.sh",
    "content": "#!/bin/bash\necho \"hello world\""
  }
}
```

#### 5. Thinking Node (child of assistant_start)
```json
{
  "type": "content_thinking",
  "thinking": "I need to make sure the script is executable..."
}
```

#### 6. Tool Result Message Node
```json
{
  "type": "user_tool_result",
  "role": "user",
  "tool_use_id": "toolu_01ABC123",
  "content": "File written successfully",
  "is_error": false
}
```

#### 7. Assistant Summary Node (end of turn)
```json
{
  "type": "assistant_complete",
  "role": "assistant",
  "usage": {
    "input_tokens": 234,
    "output_tokens": 567,
    "cost_usd": 0.0123
  }
}
```

### Tree Structure Example

```
root (text: "")
├─ user_msg_1 (text: {"type": "user_message", "content": "Write a bash script"})
│  └─ assistant_turn_1 (text: {"type": "assistant_start"})
│     ├─ content_1 (text: {"type": "content_text", "text": "I'll create..."})
│     ├─ tool_use_1 (text: {"type": "content_tool_use", "name": "Write", ...})
│     ├─ tool_result_1 (text: {"type": "user_tool_result", "tool_use_id": "toolu_01ABC123", ...})
│     ├─ content_2 (text: {"type": "content_text", "text": "Now let me..."})
│     ├─ tool_use_2 (text: {"type": "content_tool_use", "name": "Bash", ...})
│     ├─ tool_result_2 (text: {"type": "user_tool_result", ...})
│     └─ assistant_complete (text: {"type": "assistant_complete", "usage": {...}})
├─ user_msg_2 (text: {"type": "user_message", "content": "Can you make it shorter?"})
│  └─ assistant_turn_2 (text: {"type": "assistant_start"})
│     └─ ...
```

**Key Design Decisions:**

1. **Flat tool results**: Tool results are siblings of tool uses (not children), because they're logically user messages
2. **Turn grouping**: Each assistant turn has an explicit start/complete boundary
3. **Content ordering**: Child ordering within a turn preserves event sequence
4. **Self-describing**: Each node has enough info to reconstruct its part of the API message

---

## Rendering Algorithm

### Input
- `tree_id`: The arbor tree
- `start_node`: Where to begin reading (default: root)
- `end_node`: Where to stop (default: head)

### Output
Claude API messages array

### Algorithm

```rust
pub async fn render_messages(
    arbor: &ArborStorage,
    tree_id: &TreeId,
    start: &NodeId,
    end: &NodeId,
) -> Result<Vec<ClaudeMessage>, Error> {
    // 1. Get path from start to end
    let path = arbor.node_get_path(tree_id, end).await?;
    let nodes: Vec<Node> = path.into_iter()
        .skip_while(|n| n.id != *start)
        .collect();

    // 2. Group into messages
    let mut messages = Vec::new();
    let mut current_message: Option<ClaudeMessage> = None;
    let mut current_content: Vec<ContentBlock> = Vec::new();

    for node in nodes {
        let event: NodeEvent = serde_json::from_str(&node.content)?;

        match event.type {
            "user_message" => {
                // Flush previous message
                if let Some(msg) = current_message.take() {
                    messages.push(msg);
                }
                current_message = Some(ClaudeMessage {
                    role: "user",
                    content: vec![ContentBlock::Text { text: event.content }],
                });
            }
            "assistant_start" => {
                // Start new assistant message
                if let Some(msg) = current_message.take() {
                    messages.push(msg);
                }
                current_content.clear();
            }
            "content_text" => {
                current_content.push(ContentBlock::Text { text: event.text });
            }
            "content_tool_use" => {
                current_content.push(ContentBlock::ToolUse {
                    id: event.id,
                    name: event.name,
                    input: event.input,
                });
            }
            "content_thinking" => {
                current_content.push(ContentBlock::Thinking { thinking: event.thinking });
            }
            "user_tool_result" => {
                // Tool results become user messages
                if !current_content.is_empty() {
                    messages.push(ClaudeMessage {
                        role: "assistant",
                        content: current_content.clone(),
                    });
                    current_content.clear();
                }
                messages.push(ClaudeMessage {
                    role: "user",
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: event.tool_use_id,
                        content: event.content,
                        is_error: event.is_error,
                    }],
                });
            }
            "assistant_complete" => {
                // End assistant turn
                if !current_content.is_empty() {
                    messages.push(ClaudeMessage {
                        role: "assistant",
                        content: current_content.clone(),
                    });
                    current_content.clear();
                }
            }
            _ => {} // Ignore unknown types
        }
    }

    // Flush any remaining message
    if let Some(msg) = current_message {
        messages.push(msg);
    } else if !current_content.is_empty() {
        messages.push(ClaudeMessage {
            role: "assistant",
            content: current_content,
        });
    }

    Ok(messages)
}
```

---

## Session Configuration

Beyond messages, a session needs:

```rust
pub struct SessionConfig {
    // From arbor
    pub tree_id: TreeId,
    pub head: NodeId,

    // Session params (can be stored in tree metadata or separate table)
    pub model: Model,
    pub working_dir: PathBuf,
    pub system_prompt: Option<String>,
    pub mcp_config: Option<Value>,
    pub loopback_enabled: bool,

    // For resume mode
    pub claude_session_id: Option<String>,
}
```

**Storage Options:**

1. **Tree metadata**: Store config in tree's metadata field
2. **Separate table**: Keep `claudecode_sessions` table as index
3. **Root node**: Store in root node's content

**Recommendation**: Use tree metadata for config, with `claudecode_sessions` as a cache/index.

---

## Context Windowing Functions

### 1. Last N Turns
```rust
pub async fn last_n_turns(
    tree_id: &TreeId,
    head: &NodeId,
    n: usize,
) -> Result<Vec<NodeId>, Error> {
    // Walk backwards from head, collect N user message nodes
    // Return path from earliest → head
}
```

### 2. Since Bookmark
```rust
pub async fn since_bookmark(
    tree_id: &TreeId,
    bookmark: &NodeId,
    head: &NodeId,
) -> Result<Vec<NodeId>, Error> {
    // Get path from bookmark → head
}
```

### 3. With Summarization (Future)
```rust
pub async fn with_summary(
    tree_id: &TreeId,
    from: &NodeId,
    to: &NodeId,
    summary: &str,
) -> Result<NodeId, Error> {
    // Create a new summary node: {"type": "summary", "original_range": [from, to], "summary": "..."}
    // This node can be expanded back to original on demand
    // Or used as-is for compact context
}
```

---

## Migration Path

### Phase 1: Dual Write (Week 1)
- Write events to both arbor nodes AND DB tables
- Rendering reads from arbor
- DB tables still used for queries

### Phase 2: Arbor Primary (Week 2)
- All reads from arbor
- DB becomes write-only cache
- Add arbor indexes for common queries

### Phase 3: DB Optional (Week 3)
- Make DB tables optional
- All queries via arbor
- DB only for legacy compatibility

---

## New ClaudeCode Methods

### Query Methods
```rust
#[plexus_method]
pub async fn get_tree(&self, name: String) -> GetTreeResult {
    // Returns tree_id, head, and config
}

#[plexus_method]
pub async fn render_context(
    &self,
    name: String,
    start: Option<NodeId>,
    end: Option<NodeId>,
) -> RenderResult {
    // Returns Claude API messages array
}

#[plexus_method]
pub async fn create_from_tree(
    &self,
    name: String,
    tree_id: TreeId,
    start: NodeId,
    end: NodeId,
    model: Model,
    working_dir: String,
) -> CreateResult {
    // Create new session from arbor subtree
}
```

### Summarization Methods (Future)
```rust
#[plexus_method]
pub async fn summarize_range(
    &self,
    name: String,
    from: NodeId,
    to: NodeId,
) -> SummarizeResult {
    // Ask Claude to summarize a range, replace with summary node
}

#[plexus_method]
pub async fn expand_summary(
    &self,
    summary_node: NodeId,
) -> ExpandResult {
    // Expand a summary node back to original events
}
```

---

## Files to Modify

### 1. `storage.rs`
- Add `render_messages()` function
- Add arbor query helpers

### 2. `activation.rs`
- Modify `chat()` to create arbor nodes for each event
- Add `get_tree()`, `render_context()`, `create_from_tree()` methods

### 3. `types.rs`
- Add `NodeEvent` enum for all event types
- Add `ClaudeMessage` struct matching API format
- Add `ContentBlock` enum

### 4. `executor.rs`
- Add method to launch from reconstructed messages (if needed)

---

## Testable Milestones

### Milestone 1: Type Definitions (Foundation)
**Goal**: Define all types needed for arbor event storage and rendering

**Tasks**:
1. Add `NodeEvent` enum with all event variants (types.rs:600-700)
2. Add `ClaudeMessage` struct matching Claude API format (types.rs:700-750)
3. Add `ContentBlock` enum for message content (types.rs:750-800)
4. Add `RenderResult` response types (types.rs:800-850)

**Acceptance Criteria**:
- [ ] All types compile with no warnings
- [ ] Types serialize/deserialize correctly to/from JSON
- [ ] JSON schema generation works for all new types

**Verification**:
```bash
cd /workspace/hypermemetic/plexus-substrate
cargo build --features claudecode
cargo test claudecode::types::node_event -- --nocapture
```

**Test Case**:
```rust
#[test]
fn test_node_event_serialization() {
    let event = NodeEvent::ContentText { text: "Hello".to_string() };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: NodeEvent = serde_json::from_str(&json).unwrap();
    assert_eq!(event, parsed);
}
```

---

### Milestone 2: Arbor Node Creation (Event Capture)
**Goal**: Store each Claude event as an arbor text node during chat streaming

**Tasks**:
1. Add helper function `create_event_node()` in activation.rs
2. Modify `chat()` streaming loop to create nodes for each event
3. Track current parent node ID as events flow
4. Create nodes for: content, tool_use, tool_result, thinking

**Acceptance Criteria**:
- [ ] Each ChatEvent creates exactly one arbor node
- [ ] Nodes form a sequential chain (each child of previous)
- [ ] Node content is valid JSON matching NodeEvent schema
- [ ] Node creation errors don't crash the stream

**Verification**:
```bash
# Start substrate
substrate --port 4444 &

# Run test script
cd /workspace/hypermemetic/orcha-ts
npx tsx test/milestone-2-verify.ts

# Expected: arbor tree with N nodes for N events
```

**Test Script** (`test/milestone-2-verify.ts`):
```typescript
// Create session, send prompt with tool use, verify arbor nodes
const cc = createClaudecodeClient(client);
const created = await cc.create('sonnet', 'test', '/tmp', false, null);
const arbor = createArborClient(client);

// Chat with a tool-using prompt
for await (const event of cc.chat('test', 'write hello.txt')) {
  if (event.type === 'complete') break;
}

// Get session config to find tree_id
const session = await cc.get('test');
const { treeId, nodeId } = session.config.head;

// Walk tree and verify nodes
const path = await arbor.nodeGetPath(nodeId, treeId);
assert(path.length > 3, 'Should have user + events + assistant nodes');

// Verify node content is valid JSON
for (const node of path.nodes) {
  const parsed = JSON.parse(node.content);
  assert(parsed.type, 'Node should have type field');
}
```

---

### Milestone 3: Message Rendering (Read Path)
**Goal**: Walk arbor tree and produce valid Claude API messages array

**Tasks**:
1. Implement `render_messages()` in storage.rs
2. Handle all NodeEvent types correctly
3. Group events into Claude message format
4. Handle tool results as separate user messages

**Acceptance Criteria**:
- [ ] Rendering produces valid Claude API messages structure
- [ ] User/assistant turns are correctly grouped
- [ ] Tool uses appear in assistant messages
- [ ] Tool results appear as separate user messages
- [ ] Content blocks preserve order

**Verification**:
```bash
cargo test claudecode::storage::test_render_messages -- --nocapture
```

**Test Case**:
```rust
#[tokio::test]
async fn test_render_messages() {
    // Create mock arbor tree with known structure:
    // root -> user -> content -> tool_use -> tool_result -> content -> assistant
    let arbor = setup_test_arbor().await;
    let tree_id = create_test_tree(&arbor).await;

    let messages = render_messages(&arbor, &tree_id, &root, &head).await.unwrap();

    assert_eq!(messages.len(), 3); // user, assistant with tool, user with result
    assert_eq!(messages[0].role, "user");
    assert_eq!(messages[1].role, "assistant");
    assert_eq!(messages[1].content.len(), 2); // text + tool_use
    assert_eq!(messages[2].role, "user");
    assert_eq!(messages[2].content[0].content_type, "tool_result");
}
```

---

### Milestone 4: Query Methods (API Surface)
**Goal**: Expose tree info and rendering via RPC methods

**Tasks**:
1. Add `get_tree()` method to claudecode activation
2. Add `render_context()` method with optional start/end
3. Update method schemas

**Acceptance Criteria**:
- [ ] `get_tree(name)` returns tree_id and head
- [ ] `render_context(name)` returns Claude messages array
- [ ] `render_context(name, start, end)` renders subtree
- [ ] Methods appear in substrate schema

**Verification**:
```typescript
// test/milestone-4-verify.ts
const cc = createClaudecodeClient(client);

// Create and chat
await cc.create('sonnet', 'test', '/tmp', false, null);
for await (const event of cc.chat('test', 'Say hello')) {
  if (event.type === 'complete') break;
}

// Query tree info
const tree = await cc.getTree('test');
assert(tree.type === 'ok');
assert(tree.treeId);
assert(tree.head);

// Render context
const rendered = await cc.renderContext('test');
assert(rendered.type === 'ok');
assert(rendered.messages.length > 0);
assert(rendered.messages[0].role === 'user');
```

**CLI Verification**:
```bash
# Via synapse
synapse -H 127.0.0.1 -P 4444 substrate -i | jq '.methods[] | select(.name | contains("claudecode"))'
# Should show: claudecode.get_tree, claudecode.render_context
```

---

### Milestone 5: End-to-End with Orcha (Integration)
**Goal**: Orcha can query full conversation history from arbor

**Tasks**:
1. Update orcha to call `get_tree()` after task completes
2. Walk arbor tree and display all events
3. Verify tool uses, tool results, content all present

**Acceptance Criteria**:
- [ ] Orcha can fetch tree_id for a session
- [ ] Orcha can render full conversation from arbor
- [ ] All tool uses appear in rendered output
- [ ] All tool results appear in rendered output
- [ ] Content chunks are preserved

**Verification**:
```bash
# Run orcha with a tool-using task
cd /workspace/hypermemetic/orcha-ts
npx tsx src/index.ts --substrate ws://127.0.0.1:4444 <<EOF
write hello.txt with "hello world"

EOF

# After task completes, query arbor via the session name
npx tsx test/milestone-5-verify.ts
```

**Test Script** (`test/milestone-5-verify.ts`):
```typescript
// Get most recent orcha session
const cc = createClaudecodeClient(client);
const list = await cc.list();
const orchaSessions = list.sessions.filter(s => s.name.startsWith('orcha-'));
const latest = orchaSessions.sort((a, b) => b.createdAt - a.createdAt)[0];

// Render context
const rendered = await cc.renderContext(latest.name);
assert(rendered.type === 'ok');

// Verify structure
const messages = rendered.messages;
let foundToolUse = false;
let foundToolResult = false;

for (const msg of messages) {
  if (msg.role === 'assistant') {
    for (const block of msg.content) {
      if (block.type === 'tool_use') foundToolUse = true;
    }
  }
  if (msg.role === 'user') {
    for (const block of msg.content) {
      if (block.type === 'tool_result') foundToolResult = true;
    }
  }
}

assert(foundToolUse, 'Should find at least one tool_use');
assert(foundToolResult, 'Should find at least one tool_result');

// Display full conversation
console.log(JSON.stringify(messages, null, 2));
```

**Success Metrics**:
- Tool uses present: ✅
- Tool results present: ✅
- Content preserved: ✅
- Order correct: ✅

---

### Milestone 6: Session from Arbor (Write Path)
**Goal**: Create new Claude session from any arbor subtree

**Tasks**:
1. Add `create_from_tree()` method
2. Render messages from specified subtree
3. Launch Claude with reconstructed history
4. Store new session config

**Acceptance Criteria**:
- [ ] `create_from_tree(name, tree_id, start, end, ...)` creates valid session
- [ ] New session can continue conversation from that point
- [ ] Chat works correctly with reconstructed context
- [ ] New session gets unique name and ID

**Verification**:
```typescript
// test/milestone-6-verify.ts
const cc = createClaudecodeClient(client);

// Create original session and chat
await cc.create('sonnet', 'original', '/tmp', false, null);
for await (const event of cc.chat('original', 'Count to 3')) {
  if (event.type === 'complete') break;
}

// Get tree info
const tree = await cc.getTree('original');
const path = await arbor.nodeGetPath(tree.head, tree.treeId);

// Find the user message node (second node in path)
const startNode = path.nodes[1].id;

// Create new session from subtree
const created = await cc.createFromTree(
  'forked',
  tree.treeId,
  startNode,
  tree.head,
  'sonnet',
  '/tmp'
);
assert(created.type === 'created');

// Continue conversation in new session
let response = '';
for await (const event of cc.chat('forked', 'Continue to 5')) {
  if (event.type === 'content') response += event.text;
  if (event.type === 'complete') break;
}

// Verify continuation makes sense
assert(response.includes('4') || response.includes('5'));
```

---

### Milestone 7: Context Windowing (Advanced Queries)
**Goal**: Implement last_n_turns and since_bookmark functions

**Tasks**:
1. Add `last_n_turns()` helper in storage.rs
2. Add `since_bookmark()` helper
3. Integrate with `render_context()` method
4. Add `create_with_windowing()` convenience method

**Acceptance Criteria**:
- [ ] `render_context(name, {last_n_turns: 3})` returns last 3 exchanges
- [ ] `render_context(name, {since: node_id})` renders from bookmark
- [ ] Windowing preserves message structure
- [ ] Edge cases handled (n > total turns, invalid bookmark)

**Verification**:
```typescript
// test/milestone-7-verify.ts
const cc = createClaudecodeClient(client);

// Create session with multiple turns
await cc.create('sonnet', 'multi', '/tmp', false, null);
await chatAndWait(cc, 'multi', 'Say hello');
await chatAndWait(cc, 'multi', 'Count to 3');
await chatAndWait(cc, 'multi', 'Say goodbye');

// Render last 1 turn only
const windowed = await cc.renderContext('multi', {lastNTurns: 1});
assert(windowed.messages.length <= 2); // user + assistant
assert(windowed.messages.some(m => m.content.includes('goodbye')));
assert(!windowed.messages.some(m => m.content.includes('hello')));

// Render with bookmark
const tree = await cc.getTree('multi');
const path = await arbor.nodeGetPath(tree.head, tree.treeId);
const bookmark = path.nodes[4].id; // After first exchange

const fromBookmark = await cc.renderContext('multi', {since: bookmark});
assert(!fromBookmark.messages.some(m => m.content.includes('hello')));
assert(fromBookmark.messages.some(m => m.content.includes('goodbye')));
```

---

## Milestone Summary Table

| # | Milestone | Deliverable | Verification Command | Status |
|---|-----------|-------------|---------------------|--------|
| 1 | Type Definitions | NodeEvent, ClaudeMessage, ContentBlock types | `cargo test claudecode::types::node_event` | ⬜ |
| 2 | Arbor Node Creation | Events stored as arbor nodes during chat | `npx tsx test/milestone-2-verify.ts` | ⬜ |
| 3 | Message Rendering | render_messages() function | `cargo test claudecode::storage::test_render_messages` | ⬜ |
| 4 | Query Methods | get_tree(), render_context() RPC methods | `npx tsx test/milestone-4-verify.ts` | ⬜ |
| 5 | End-to-End with Orcha | Full conversation visible in arbor | `npx tsx test/milestone-5-verify.ts` | ⬜ |
| 6 | Session from Arbor | create_from_tree() method | `npx tsx test/milestone-6-verify.ts` | ⬜ |
| 7 | Context Windowing | last_n_turns(), since_bookmark() | `npx tsx test/milestone-7-verify.ts` | ⬜ |

---

## Success Criteria (Final Acceptance)

✅ **M1-M3**: Arbor tree contains complete conversation history
```bash
# After any claudecode chat:
sqlite3 arbor.db "SELECT COUNT(*) FROM nodes WHERE tree_id = '{tree}';"
# Should equal: 1 (root) + 1 (user) + N (events) + 1 (assistant)
```

✅ **M4**: Query methods expose arbor data
```bash
# Via substrate client:
substrate.claudecode.get_tree("session") → {tree_id, head}
substrate.claudecode.render_context("session") → [{role, content}, ...]
```

✅ **M5**: Orcha can inspect full conversation
```typescript
const rendered = await cc.renderContext(sessionName);
assert(rendered.messages.every(m => m.role && m.content));
```

✅ **M6**: New session created from arbor subtree continues correctly
```typescript
const created = await cc.createFromTree(...subtree...);
await cc.chat(created.name, "continue") // Works with prior context
```

✅ **M7**: Context windowing produces valid, smaller message arrays
```typescript
const all = await cc.renderContext(name);
const windowed = await cc.renderContext(name, {lastNTurns: 2});
assert(windowed.messages.length < all.messages.length);
```

---

## Open Questions

1. **Tool result attribution**: Should tool results be children of their tool_use, or siblings?
   - **Decision**: Siblings, because they're logically new user messages

2. **Streaming chunks**: Store every content delta as a node, or accumulate?
   - **Decision**: Store deltas — faithful recording, can aggregate on render

3. **Metadata storage**: Where to store session config (model, working_dir)?
   - **Decision**: Tree metadata + optional DB cache

4. **Claude session ID**: Still track for resume mode?
   - **Decision**: Yes, store in tree metadata for efficiency

5. **DB tables**: Keep or remove?
   - **Decision**: Phase out gradually (dual write → read-only → optional)
