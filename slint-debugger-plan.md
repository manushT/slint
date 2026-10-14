# Splitting the Slint Debugger Into Pull Requests

Status as of 2026-10-16.
Scope: a breakdown of issue [#14079](https://github.com/slint-ui/slint/issues/14079), "Tracking: Slint Debugger", into items that each fit a pull request.
The MCP work in [slint-mcp-improvements.md](slint-mcp-improvements.md) builds on top of it.

## Summary

- The debugger's MVP inspects a running preview from the Springboard, read-only.
  Its features are console output with preview diagnostics, an outline of the running item tree, a property inspector, and Inspect mode.
- Console output and diagnostics already reach the Springboard.
  They come first, since they need no runtime work.
- The outline builds on the interpreter's highlight code.
  Reading every property's value and picking elements with the mouse need the most new code.
- One security fix comes before everything else: today a remote viewer can read any file through the editor.
- Four decisions shape the protocol and the UI, see [Decisions to Settle First](#decisions-to-settle-first).

Sizes are small, medium or large, and each numbered item is one pull request.

## What Exists Today

| Feature | Exists | Missing |
| --- | --- | --- |
| Console output | `debug()` output and runtime warnings reach the Springboard as `DebugMessage` | A view; the editor prints them to its stderr |
| Preview diagnostics | The run preview sends `Diagnostics` | A view; the editor drops them |
| Outline | `highlight.rs` maps runtime items to source locations, and the interpreter always compiles with debug info | A tree walk that returns nodes, popups, protocol messages, a view |
| Property inspector | `@testable` values, not on `master` yet | Every other property; the compiler optimizes many away |
| Inspect | `element_candidates_at` finds source elements under a point, and `InspectorOverlay` highlights them | Input while picking, selection filters that work in the run child, protocol messages |

The removed Springboard design document lists the same requirements.
It defers "runtime inspection messages, capabilities, and data ownership".
See `git show 286cee31ea^:internal/editor-preview/springboard/ARCHITECTURE.md`.

## Before the Debugger

### P1. Stop Remote Viewers From Reading Arbitrary Files

A remote viewer asks for files with `RequestState`, and the Springboard forwards it to the editor.
The editor answers with `send_files_to_preview(preview_index, files, |_| true)`, see [`main.rs`](tools/editor/main.rs#L506).
So a viewer connected through the Springboard can read any file the user can read.
The LSP filters the same request with `PreviewFileAccess`, see [`preview_files.rs`](tools/lsp/language/preview_files.rs).
Share that filter with the editor.

This fix stands on its own.
The debugger only adds more traffic from viewers.

Size: small.

### P2. Test Protocol Messages in JSON and Postcard

No test serializes the message enums and reads them back.
The local run child speaks JSON, and remote viewers speak postcard, which isn't self-describing.
`lsp_types::Diagnostic` uses `skip_serializing_if` and untagged enums, and postcard can't round-trip either.
The remote tests send only empty diagnostics, so non-empty ones may fail to decode.
We haven't confirmed that.
The same tests then guard the debugger's new messages.

Size: small.

### P3. Share One Debug Agent Between the Run Child and the Viewer

The run child's loop in [`preview_sessions.rs`](internal/live-preview/preview_sessions.rs) and the remote viewer's loop in [`remote.rs`](tools/viewer/remote.rs) are near-duplicates.
Each debugger request would need a handler in both.
Move the handling into one place that both loops call.
It's a refactor without new behavior.

Size: small to medium.

### P4. Keep Debugger Messages in the Springboard

Remote viewers may send only `Diagnostics`, `DebugMessage` and `RequestState`, see [`remote_client.rs`](internal/editor-preview/remote_client.rs#L15-L22).
The LSP's connector uses the same list, so debugger messages added there would reach the LSP.
Add a second category that the Springboard consumes itself and never passes to the editor.
Limit message sizes and rates, since a remote viewer is untrusted.

Size: small.

## Milestone 1: Console and Diagnostics

### 1. Add the Debugger Panel With a Console

The Springboard is a 260×300 overlay below the editor's top bar, see [`main.slint`](tools/editor/ui/main.slint#L661-L667).
Give the debugger a larger home that shows once the Springboard connects.
Its first content is the console.
It shows the `DebugMessage` lines that the editor prints to its stderr today, see [`main.rs`](tools/editor/main.rs#L543-L545).
Keep a bounded buffer, and mark where each rebuild starts.

This item settles where the debugger lives, see [Decisions to Settle First](#decisions-to-settle-first).

Size: medium.

### 2. Tell `debug()` Output From Runtime Warnings

[`install_debug_handler`](internal/live-preview/preview_sessions.rs#L642-L660) drops the message's `LogMessageSource`.
So `debug()` output and the runtime's own warnings look alike.
Only the missing location hints at the difference.
Add the source to `DebugMessage`, so that the console can label and filter both.

Size: small.

### 3. Open a Console Location in the Editor

`debug()` output carries a file, line and column.
Clicking one shows it in the editor.
Accept only paths inside the project, since a remote viewer chooses what it sends.

Size: small.

### 4. Show Preview Diagnostics

The run preview sends `Diagnostics`, and the editor drops them, see [`main.rs`](tools/editor/main.rs#L577).
Show them in the debugger next to the console.
Remote diagnostics depend on P2.

Size: small.

### 5. Report Crashes in the Console

The run child inherits the editor's stderr, so a panic only shows up there, see [`child_process.rs`](internal/editor-preview/child_process.rs#L47-L51).
The editor itself only reports "The Slint live preview crashed!".
A binding loop at runtime is such a panic, see "Recursion detected" in [`properties.rs`](internal/core/properties.rs#L651).
Capture the child's stderr, and show its last lines in the console.

Size: small.
It's optional for the MVP.

## Milestone 2: Outline

### 6. Walk a Running Instance in the Interpreter

Add an interpreter API that returns the running item tree as nodes.
Each node has a type name, a qualified id such as `App::ok-button`, a source location, its repeater row, and its children.

The pieces exist:

- The interpreter always compiles with debug info, see [`component.rs`](internal/interpreter/component.rs#L459).
- [`highlight.rs`](internal/interpreter/highlight.rs) maps runtime items to source locations through the compiled form.
- Each item's element infos hold its type names and qualified ids.

Use only the compiled form.
The run child doesn't keep the object tree, because `into_send()` drops the type loader.
See [`preview_sessions.rs`](internal/live-preview/preview_sessions.rs#L439) and [`api.rs`](internal/interpreter/api.rs#L1197).

Size: medium.

### 7. Include Popups and Component Containers

[`all_instances`](internal/interpreter/highlight.rs#L525) follows repeaters and sub-components only.
Popups are separate item trees, which the window lists in `active_popups()`.
A false `if` or an empty `for` leaves no runtime item.
The compiled form's `repeated_elements` still knows its source, if the outline should show it.

Size: small to medium.

### 8. Request the Tree Over the Protocol

The protocol has no request ids, and every message is fire-and-forget.
Add a request for the tree, and a reply with a full snapshot.
Use postcard-safe types: no `skip_serializing_if`, untagged enums, `flatten` or `serde_json::Value`.

Size: small.

### 9. Add the Outline View

Show the tree, with a Refresh button.
Selecting a node highlights it in the viewer through the existing `HighlightFromEditor` message.

Size: medium.

### 10. Highlight a Single Repeater Row

`HighlightFromEditor` carries a source offset, so the viewer highlights every instance of that element.
Add the instance, so that selecting a row in the outline highlights only that row.

Size: small.

### 11. Refresh the Outline Automatically

Refresh after a rebuild, when a repeater's model changes, and when a popup opens.

Size: medium.
It can come after the MVP.

## Milestone 3: Property Inspector

### 12. Read Built-in Properties at Runtime

The interpreter reads a built-in item's property by name, see `ErasedItem::get_property` in [`erased.rs`](internal/interpreter/erased.rs#L21).
It can't list the names, so take them from the item's `NativeClass` in the compiled form.
One source element can span several items, such as an opacity or clip wrapper.
Merge them into one node.

Size: medium.

### 13. Read Declared Properties at Runtime

The compiler changes declared properties before they run:

- It removes properties that nothing reads.
- It folds constant bindings.
- It merges aliases, and moves declarations to their component's root.

Add a compiler table from each element to the properties that survive.
The `@testable` table, which isn't on `master` yet, is a model for it.
Show folded and removed properties as such, instead of hiding them.

Size: large, and the riskiest item.
Spike it first, since it may split further.

### 14. Encode Property Values for the Protocol

Start with a display string and the type name for each value.
Structs, models, images and gradients can follow.

Size: small.

### 15. Add the Inspector View

Show the properties of the node selected in the outline, with a Refresh button.
Keep it read-only.
Don't build on the editor's debug hooks, which override values.

Size: medium.

## Milestone 4: Inspect

### 16. Let the Debugger Take Input While Picking

Overlays don't take input, see `add_overlay` in [`window.rs`](internal/core/window.rs#L2351-L2354).
The window event hook runs after dispatch, so it can't stop a click from reaching the app.
Add a way for the overlay to take pointer events while Inspect is on.

Size: medium, in core.

### 17. Pick Elements in the Viewer

`ComponentInstance::element_candidates_at` returns the source elements under a point, and it works in the run child, see [`api.rs`](internal/interpreter/api.rs#L1886).
The editor's selection filters need the object tree, see [`element_selection.rs`](tools/editor/preview/element_selection.rs).
Port them to the compiled form, or keep the type loader in the run child.
Highlight the element under the mouse while Inspect is on.

Size: medium.

### 18. Add Selection Mode to the Protocol

Add a message that turns selection mode on and off, and one that reports the picked element.
A remote viewer's pick mustn't move the editor's caret unless its URL is inside the project.
Issue #12876 blocked remote `ShowDocument` for that reason.

Size: small.

### 19. Add the Inspect Button

A pick selects the element in the outline and in the inspector.
The ticket notes that Inspect may not work in compiled apps.

Size: small.

## Beyond the MVP

Each of these deserves its own ticket.

- **"Show Preview" launches the Springboard.**
  The LSP starts its own preview today, see [`main.rs`](tools/lsp/main.rs#L315).
  The LSP's main process has no Slint event loop, so this needs a standalone Springboard host.
  Viewer discovery exists three times and the connection loop twice, which this can merge.
- **SlintPad.**
  The Springboard doesn't build for wasm, see [`lib.rs`](internal/editor-preview/lib.rs#L21-L26).
- **Compiled apps.**
  The system-testing transport in [`systest.rs`](internal/backends/testing/systest.rs) is a plausible base: the app connects out and answers requests.
  Compiled apps carry no source locations, and their debug info is off by default.
- **Multiple viewers, and logging in to a Slint account.**
  There's no code for either.

## Decisions to Settle First

- **Where the debugger lives.**
  It can be a panel in the editor window, or a window of its own.
  `Springboard::new` takes a single UI global, which ties it to one window.
- **Pull or push.**
  We suggest pull first: the Springboard requests a snapshot and gets it back.
  Push needs change tracking in the viewer.
- **Outline nodes.**
  There can be one node per runtime instance, or one per source element with an instance count.
- **The object tree in the run child.**
  Keeping it simplifies picking and property lists, but costs memory.
  The run child drops it so that a compiled preview can move between threads.

## Suggested Order

| Step | Items | Gain |
| --- | --- | --- |
| 1 | P1, P2 | Remote viewers can't read arbitrary files, and the protocol has tests |
| 2 | 1, 2, 4 | A visible debugger with console and diagnostics |
| 3 | 3, 5 | Console locations open in the editor, and crashes show up |
| 4 | P3, P4, 6–10 | Outline |
| 5 | 12–15 | Property inspector, after the spike on 13 |
| 6 | 16–19 | Inspect |
| 7 | 11 | An outline that stays current |

Items 6, 12 and 16 touch only the interpreter or core.
They can start while milestone 1 is in review.

## The MCP Work on Top

[slint-mcp-improvements.md](slint-mcp-improvements.md) plans MCP tools for each debugger feature.
Keep each feature's state as plain Rust data in the Springboard, not only in Slint models.
Then the tools return the same data that the view draws.
