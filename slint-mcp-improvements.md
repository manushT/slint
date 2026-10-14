# Improving Slint's MCP Server for Models

Status as of 2026-10-16.
Scope: changes to Slint's in-app MCP server, the visual editor and its Springboard that make it easier and cheaper for models to drive a Slint UI.

A front-end like KDGuiTestMCP's `guitest-mcp` isn't planned for Slint any time soon.
So these changes go into Slint itself, where every application and every MCP client gets them.
[slint-gui-test-evaluation.md](slint-gui-test-evaluation.md) covers the server's design and the changes already done.

## Summary

- We want models to use a UI through the visual editor: open the `.slint` file in `slint-editor` and press Run.
  That doesn't work over MCP yet, because the running app can't bind the MCP port.
  Two small editor changes fix it, see [The Editor Workflow](#the-editor-workflow).
- Most round trips go away when action tools accept a query, a `wait_for` tool replaces polling, and keys can be named.
- Most tokens go away with snapshot diffs after actions, a leaner snapshot and a shorter tool list.
  The tool list alone costs about 5.4k tokens in every session.
- The Springboard will get a read-only debugger, tracked in issue #14079.
  Serve its data as MCP tools from the Springboard, which holds the connection to the viewer and outlives each Run.
  See [The Debugger in the Springboard](#the-debugger-in-the-springboard).

## How We Measured

We built the editor with the MCP server and opened [`gui-test-sample/app_testable.slint`](gui-test-sample/app_testable.slint):

```sh
cargo build -p slint-editor --features slint/mcp --target-dir target/mcp
SLINT_ENABLE_EXPERIMENTAL_FEATURES=1 SLINT_MCP_PORT=8080 \
    target/mcp/debug/slint-editor gui-test-sample/app_testable.slint
```

A small JSON-RPC client then called the tools over HTTP.
Token counts are estimates at four bytes per token.

| What | Size |
| --- | --- |
| Tool list | 20 tools, 21.8 KB, about 5.4k tokens; `query_element_descendants` alone is 3.5 KB |
| Calls before the first snapshot | 3: `list_windows`, `get_window_properties`, `get_element_snapshot` |
| Snapshot of the editor window | 120 lines, 7.7 KB; rectangles take 25%, handles 9%, indentation 8%, ids 7% |
| Screenshot of the editor window | 1700×1075 physical pixels, 148 KB of PNG |
| Snapshot of the sample in a 400×500 window | 1.3 KB, where `get_element_tree` returns 42 KB (from the evaluation) |

## The Editor Workflow

The workflow we want:

1. Open the UI in `slint-editor`, with `SLINT_ENABLE_EXPERIMENTAL_FEATURES=1` so that `@testable` compiles.
2. Press Run.
   The editor opens a Springboard panel, which lists "Local" and the remote viewers it finds on the network.
   Picking "Local" starts the app as `slint-editor --run-preview-child`, in a process of its own.
3. The model drives the running app over MCP.

Steps 1 and 2 work over MCP.
Run is a `TouchArea role=Button "Run"` in the editor's top bar, and "Local" is a `Button` in the Springboard panel.

Step 3 fails.
The child inherits the editor's environment, including `SLINT_MCP_PORT`, and the editor already holds that port:

```text
MCP server: failed to bind to 127.0.0.1:8080: Address already in use (os error 98)
```

A client on port 8080 drives the editor's own UI, not the app.

### 1. Give the Run Child Its Own Port

In [`child_process.rs`](internal/editor-preview/child_process.rs), remove `SLINT_MCP_PORT` from the child's environment.
Set it from a separate variable instead, such as `SLINT_EDITOR_RUN_MCP_PORT`.
Port 0, with the bound port reported back, would work as well; it's item 3 in the evaluation.

Size: small.
Nothing else in the workflow works without it.

### 2. Add `slint-editor --run`

A flag that starts the run preview at startup.
Without it, a model clicks Run and then "Local", and reads the editor's 7.7 KB snapshot to find them.

Size: small.

### 3. Make the Springboard's MCP Server the Stable Endpoint

Every Run restarts the child, and with it the child's MCP server.
Claude Code then marks the server failed until the user reconnects with `/mcp`.
The Springboard outlives every Run and every switch to another viewer.
It runs in the editor process, so the editor's server can stay connected:

- It offers Springboard tools: list the viewers, connect to one, and disconnect.
- It forwards the app's tools to the connected viewer.
  For a remote viewer, that needs MCP requests to travel over the preview protocol.
- Later, it offers the debugger's tools, see [The Debugger in the Springboard](#the-debugger-in-the-springboard).

This needs an extension point in the in-app server, so that a host program can register its own tools.
Today the server has a fixed list of 20.

Size: large.
It takes over what a front-end like `guitest-mcp` would do.

## Fewer Calls

### 4. Let Action Tools Take a Query

`click_element` and the other action tools take only an `elementHandle`.
Let them take a `queryStack` instead, as `query_element_descendants` does.
With exactly one match, the tool acts; otherwise it fails and lists the matches.
That turns a query plus an action into one call.
It's the in-app version of KDGuiTestMCP's locators.

Size: medium.

### 5. Add a `wait_for` Tool

Today a model checks the effect of an action by taking snapshots until it sees the change.
A `wait_for` tool polls a query until it matches, or until it stops matching, with a 5 s default timeout.
It optionally checks an accessible value or a `@testable` value.
Tool handlers are already async: `click_element` awaits `ElementHandle::single_click()`.

Size: medium.

### 6. Accept Key Names in `dispatch_key_event`

`dispatch_key_event` sends its text as a single key event.
Return is `"\n"`, and the arrow keys are private-use code points such as `"\u{F702}"`, see [`key_codes.rs`](internal/common/key_codes.rs).
Models don't know these.
Accept names such as `"Return"` and `"Left"`, and combinations such as `"Ctrl+A"`.

Size: small to medium.

### 7. Snapshot Every Window Without Arguments

Done in "testing/mcp: list the UI compactly with get_element_snapshot".
`maxElements` applies per window, and the server instructions now start their workflow with this call.

`get_element_snapshot` needed an element handle, and getting one takes `list_windows` and `get_window_properties`.
Without arguments, it should list every window, as the `snapshot` argument of action tools already does.

Size: small.

## Fewer Tokens

### 8. Return Snapshot Diffs After Actions

With `snapshot: true`, an action returns the full snapshot of every window.
After a click on Increment in the sample, about one line changes.
Give each snapshot an id, and let actions return only the lines that changed since that snapshot.
The server keeps no per-session state today, so it has to start keeping the last snapshot.

Size: medium.

### 9. Trim the Snapshot

Done in "testing/mcp: list the UI compactly with get_element_snapshot".
`rectangles: true` adds the rectangles back.
The snapshot of the sample in a 400×500 window shrank from 1.3 KB to 0.9 KB.
The query for the label "Run" still matches both elements.

- Make rectangles opt-in.
  They're a quarter of the editor's snapshot, and models act by handle.
- Leave out a `Text` whose string repeats its listed parent's label.
  The editor's Run button shows up twice, as `TouchArea role=Button "Run"` and as `Text "Run"`.
  A query for the label "Run" matches both, so the model has to add the role.

Size: small.

### 10. Shorten the Tool List

The list costs about 5.4k tokens before a model does anything.

- `query_element_descendants` spends most of its 3.5 KB on the matcher grammar.
  That fits in the schema's field descriptions.
  Done in "testing: match queries by accessible strings and @testable values".
  The tool shrank only from 3389 to 3082 bytes, since most of what's left is the schema itself.
  The server instructions shrank from 7143 to 6189 bytes, as their copy of the grammar went too.
- The `elementHandle` description is 425 bytes, and about 14 tools repeat it.
- `find_elements_by_id` is covered by `query_element_descendants`.
- `move_pointer` can merge into `hover_element`, and `dispatch_pointer_scroll` into `scroll_element`.
  The merged tools take a handle or a position.
- `start_event_recording` and `stop_event_recording` are diagnostics that most sessions never use.

Size: small, though it touches the documentation and tests.

### 11. Accept Short Handles

The snapshot prints a handle as `[124:1]`, but every tool takes `{"index": "124", "generation": "1"}`.
Accepting `"124:1"` saves the conversion and about 30 characters per handle.
This matters less once action tools take queries.

Size: medium, since it changes every tool's input schema.

### 12. Limit Screenshot Size

A screenshot of the editor is 148 KB of PNG, roughly 2k tokens once the client scales it down.
Add a maximum size argument, and crops to a single element, which issue #13252 tracks.

Size: small to medium.

## Clearer State

### 13. Show Disabled TouchAreas

Done for the snapshot in "testing/mcp: list the UI compactly with get_element_snapshot".
The snapshot reads `enabled` of a `TouchArea` or gesture handler itself.
Screen readers still see a `TouchArea` with a role as enabled.
Binding `accessible-enabled` in the compiler would fix both, in a pull request of its own.

The snapshot prints `disabled` when an element's accessible enabled state is false.
The compiler binds `accessible-enabled` to `enabled` only for `TextInput`, see [`lower_accessibility.rs`](internal/compiler/passes/lower_accessibility.rs).
A `TouchArea` with `enabled: false` therefore looks enabled.
The editor's Run button is such a `TouchArea`, so a model can't tell when Run is unavailable.

Size: small.

### 14. Make Errors Say What to Do Next

- For a stale handle, say that the element is gone and to query again.
- For a query without a match, list the closest candidates, such as elements with a similar label.

Size: small.

## The Debugger in the Springboard

Issue #14079 tracks a debugger, "Gammaray for Slint".
It's a view that the Springboard shows once it connects to a viewer.
As of 2026-10-16 there's no code for it yet.

- The MVP inspects a running preview, read-only.
- A later version connects to any Slint app that contains debug info.
- The Springboard and its debugger replace the LSP's live preview, and "Show Preview" launches them.
  So the editor's Run and an IDE's "Show Preview" both lead to the Springboard.

Models should be able to read the debugger's data as well.
Reading the debugger's view through a snapshot costs more, and turns structured data into UI text.

### Tools for Each Feature

| Debugger feature | Over MCP today | Tool for models |
| --- | --- | --- |
| Inspect, which puts the viewer into selection mode | Nothing, and the preview protocol has no message for an element picked in a viewer | Return the element the user picked, so that they can point at it instead of describing it |
| Console output | Nothing; `debug()` output reaches the Springboard, and the editor prints it to its stderr | Return the lines since the last call |
| Preview diagnostics | Nothing; they reach the Springboard, and the editor drops them | Return the current diagnostics |
| Outline of the running item tree | `get_element_snapshot`, from the app's own server | The same tree, in the snapshot's format |
| Property inspector | `get_element_properties`: geometry, accessible properties and `@testable` values | Every property's value at runtime |

Console output and diagnostics need only the tools, not the debugger's view.
The ticket says Inspect may not work in compiled apps.

### Where the Tools Run

Serve the tools from the process that hosts the Springboard, not from the viewer.
Today that's `slint-editor`, where the Springboard runs on the LSP thread, see [`main.rs`](tools/editor/main.rs).

- The Springboard outlives each Run and each switch to another viewer, see [item 3](#3-make-the-springboards-mcp-server-the-stable-endpoint).
- It already receives the viewer's diagnostics and console output, see [`springboard.rs`](internal/editor-preview/springboard.rs).
- It reaches remote viewers.
  A remote viewer's own MCP server binds to `127.0.0.1` on its device by default.
  The Springboard instead holds an authenticated WebSocket to it.

A remote viewer may send only diagnostics, console output and state requests, see [`remote_client.rs`](internal/editor-preview/remote_client.rs).
The outline and the property inspector need new messages from the viewer.
Design them once, for both the debugger's view and the tools.

### Design for Models

- Keep one state model, which the debugger's view draws and the tools return.
- Return compact text like the snapshot and the editor's console lines, such as `DEBUG main.slint:12:9> clicked`.
- Use the identifiers models already have: qualified ids such as `App::ok-button`, element handles, and `file:line:col`.
- Return console output from a tool, not as notifications.
  Models don't reliably see notifications that a server sends on its own.

### Beyond the MVP

The MVP never pauses the app.
If a later version does, its tools can't run in the viewer.
The in-app server runs on the app's event loop, see `spawn_local` in [`mcp_server.rs`](internal/backends/testing/mcp_server.rs).
A paused callback blocks that loop, so the viewer can't answer until it resumes.
Then make `continue` and `step` block with a timeout, and allow breakpoints on property changes.
The editor's debug hooks are a starting point for property watches, see [`debug_hook.rs`](internal/interpreter/debug_hook.rs).

## Other Findings

- `SLINT_BACKEND=headless` fails in `slint-viewer`, even with its `mcp` feature.
  The error is "headless backend requested but it is not available".
  The viewer selects its backend through `BackendSelector`, which has no case for it.
  The editor handles it in its own `select_backend()`.
  The evaluation said headless works.
  It now names the viewer as the exception, in "slint-gui-test-evaluation".
  The viewer itself still fails.
- The editor's two windows both report `{"index": "1", "generation": "1"}` as their root element handle.
  We haven't looked into why.
- In Claude Code's sandbox, shell commands can't reach the MCP port.
  Claude Code's own MCP client isn't affected, but scripts and CLI tools run from the shell are.

## Suggested Order

| Step | Items | Size | Gain |
| --- | --- | --- | --- |
| 1 | 1, 2 | Small | The editor workflow works at all |
| 2 | 6; 7, 9 and 13 are done | Small | Fewer calls and tokens, and no hidden disabled state |
| 3 | 4, 5 | Medium | Most round trips saved |
| 4 | 8, 10; 10's matcher grammar is done | Small to medium | Most tokens saved |
| 5 | 3 | Large | One connection across Runs and viewer switches, and a home for the debugger's tools |
| 6 | 11, 12, 14 | Small to medium | Smaller savings and clearer errors |

Each item fits in a pull request of its own.

## Open Questions

- Should the debugger's outline use the snapshot's format and element handles, so that a model can act on what it finds there?
- Should a model be able to start Inspect's selection mode, or only read what the user picked?
- Should the run child's port come from its own variable, or should port 0 come first?
- Should snapshot diffs be keyed by a snapshot id that the client passes back, or by connection?
- Once action tools take queries, are short handles still worth a schema change?
