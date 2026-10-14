# A KDGuiTestMCP-Style Test Tool for Slint: Evaluation

Status as of 2026-10-14.
Scope: what it takes to build a tool like KDGuiTestMCP for Slint applications, without its recording features.

## Summary

- Slint already has the part that runs inside the application.
  Its testing backend serves an MCP server with a tree dump, queries, screenshots and synthetic input.
- What's missing in this repository is the part that runs outside the application:
  launching, stable locators, waits, a small CLI and a test harness.
- Agents explore through the in-app server directly, without an MCP front-end like KDGuiTestMCP's.
  Without recording, a front-end adds little, see [Why No MCP Front-End](#why-no-mcp-front-end).
  Agents read a compact snapshot from Slint's server instead.
- SixtyFPS already has a private Python test driver, `slint-testing`, used by the visual editor's UI tests.
  It covers launching, queries and waits, so talk to SixtyFPS before building those again.
- Rough estimate: about one week to a usable first version, two to three weeks to match KDGuiTestMCP without recording.
  That's roughly 5–7k lines of Rust, plus five small changes to Slint, four of which are done.
- The main dependency is [PR #13372](https://github.com/slint-ui/slint/pull/13372).
  It makes properties marked `@testable` readable, so tests can check application state, not only accessibility fields.
- The same crate has an in-process testing API for Rust and C++.
  Rust tests written from an exploration can run without a separate process, using mock time instead of waits.

## KDGuiTestMCP in Brief

KDGuiTestMCP drives an unmodified Qt 6 application.
It has three layers:

- A C++ agent that Qt loads into the application through its `-testability` hook.
  It serves JSON-RPC on a unix socket: object tree, properties, synthetic input, screenshots.
- A Rust client library: launching, locators, snapshots, waits and the `gui_test!` harness.
- Two front-ends on that library: the `guitest` CLI, and `guitest-mcp`, a stdio MCP server.
  `guitest-mcp` runs before any application does and offers a `launch` tool.

The ideas worth porting:

- **Locators.** A locator is a space-separated conjunction of predicates, such as `text:Save under:saveDialog`.
  The client verifies that it matches exactly one element.
- **Naming ladder.** Six steps try the simplest unique name first.
  The order is name, label, class, ancestor scope, accessible name, and finally uniqueness among visible elements only.
- **Snapshot.** A compact table of interactive elements with locator, class, rectangle and state.
  The MCP server appends one after every action.
- **Waits.** `waitUntil`, `waitFor` and `waitGone` poll every 100 ms with a 5 s default.
  The instructions forbid sleeping.
- **Harness.** On failure, `gui_test!` writes the snapshot, a screenshot and the application log to `.guitest/<test>/`.

Sizes, from `wc -l` in the KDGuiTestMCP checkout:

| Component | Lines | Needed for Slint |
| --- | --- | --- |
| Agent (`agent/`) | ~4.7k C++ | No: Slint's testing backend covers it |
| Recording (`recorder.rs`, `script.rs`, parts of the front-ends) | ~3.4k | No: out of scope |
| Coverage tooling (`coverage.rs`, `survival.rs`, `tools/`) | ~5.4k | Later, to measure the naming ladder |
| Model-index client (`model.rs`) | ~0.7k | No: list rows are elements in Slint |
| Locating, naming, tree, snapshot | ~4k | Yes, adapted |
| App facade, transport, launcher, session, waits, keys, errors | ~4.7k | Yes, mostly simpler |
| CLI front-end (`guitest-cli`) | ~0.7k | Partly: `launch` and `snapshot` |
| MCP front-end (`guitest-mcp`), without recording | ~1k | No: agents use the in-app server, see [Why No MCP Front-End](#why-no-mcp-front-end) |
| Harness | ~0.4k | Yes |

## What Slint Already Has

The testing backend in [`internal/backends/testing/`](internal/backends/testing/) runs an MCP server inside the application.
[`docs/development/mcp-server.md`](docs/development/mcp-server.md) describes its design.

| KDGuiTestMCP | Slint today |
| --- | --- |
| C++ agent loaded through `-testability` | Built into the testing backend: window and element tracking, queries, screenshots, input |
| Tree, read property, find objects | Flat tree dump, element properties, and queries by id, type, role or label. Only accessibility fields and geometry are readable |
| Click, drag, wheel, keys, modifiers | All present |
| Model-index inspection for item views | Not needed: list rows are real elements |
| One test at a time on the display, Xvfb container | Mostly unnecessary: the headless backend and synthetic events never touch the desktop |
| Linux only (`/proc`, `LD_LIBRARY_PATH`) | Connects over localhost TCP, so macOS and Windows work too |

Enabling it:

- **Rust:** build with `--features slint/mcp` and `SLINT_EMIT_DEBUG_INFO=1`.
- **C++:** use a Slint library built with its `mcp` feature, and compile the `.slint` files with debug info.
- **Python:** install `slint-dev` next to `slint`.
- **Node:** the `mcp` feature of the Node binding.
- **`slint-viewer`:** the `mcp` feature.
  The interpreter always emits debug info, so any `.slint` file works without a rebuild.

At runtime, `SLINT_MCP_PORT=<port>` starts the server.
`SLINT_BACKEND=headless` runs without a display or GPU, except in `slint-viewer`.
The viewer selects its backend through `BackendSelector`, which has no headless case.

Slint has a second transport: system testing, which sends protobuf over TCP.
There the application connects to a server named by `SLINT_TEST_SERVER`.
The two transports share their element lookup, but differ in ways that matter to the tool:

| | MCP | System testing |
| --- | --- | --- |
| Who listens | The application, on a fixed `SLINT_MCP_PORT` | The test driver; the application connects to it |
| Tree dump | `get_element_tree` returns the whole tree in one call | Rejected as "only supported via the MCP transport" |
| Used by | AI agents, through the MCP protocol | SixtyFPS's `slint-testing`, see [The `slint-testing` Package](#the-slint-testing-package) |

With system testing, the driver binds its port before it starts the application.
That avoids a port race, and the incoming connection signals that the application is ready.
Without a tree dump, though, a snapshot needs a query plus one property read per element.
MCP's single-call tree is what snapshots and the naming ladder need.

Slint's `start_event_recording` and `stop_event_recording` are a diagnostic event log.
They don't record user actions into a transcript, so dropping recording doesn't affect them.

### How the Server Sees the App

The server never reads the fields of the generated structs; they're private, see [Generated Code](#generated-code).
It goes through `ItemTreeVTable`, the functions every generated component implements for the runtime.
Rendering, input handling and screen readers use the same functions.

| Vtable functions | What the server gets from them |
| --- | --- |
| `get_item_tree`, `visit_children_item`, `parent_node`, `get_subtree`, `ensure_instantiated` | The element tree, including the rows of `for` loops |
| `item_geometry`, plus built-in elements such as `Text` being runtime types | Size, position and opacity |
| `accessible_role`, `accessible_string_property`, `accessibility_action`, `supported_accessibility_actions` | Accessibility fields such as label, value, checked and enabled; accessibility actions and setting values |
| `item_element_infos` | Type names and ids such as `App::ok-button`, with debug info only |
| `window_adapter` | Screenshots through the renderer, and input through the normal event path |

That makes the server useful for:

- **Driving any application.**
  Synthetic clicks, typing, drags and scrolling take the same path as real input.
- **Reading what the standard widgets expose through accessibility.**
  `LineEdit` binds `accessible-value <=> text`, `CheckBox` binds `accessible-checked <=> checked`, and `Slider` binds `accessible-value: value`.
  In an application built from standard widgets, much of the visible state is readable today.
- **Structure and diagnosis:** ids, geometry, screenshots and the event log.

It can't read custom state that has no accessibility binding, such as a `selected` flag on a custom card.
That's the case from #13244, where comparing screenshots was the only option.
There are two ways to close the gap:

- **Today:** the author adds an accessibility binding, such as `accessible-item-selected: root.selected`.
  That also helps screen-reader users, and it's the same idea as Playwright preferring role and label locators.
- **With PR #13372:** two new vtable functions, written by the code generator inside the private module.
  A private field reaches the runtime only this way, so it takes a compiler change, not an MCP change.

## In-Process Testing API

The same crate, `i-slint-backend-testing`, has an API for tests that run inside the test process.
The MCP server is built on it: both use `ElementHandle`.
The crate's [README](internal/backends/testing/README.md) documents it.

```rust
i_slint_backend_testing::init_integration_test_with_system_time();
slint::spawn_local(async move {
    let app = App::new().unwrap();
    let buttons: Vec<_> = ElementHandle::find_by_accessible_label(&app, "Submit").collect();
    assert_eq!(buttons.len(), 1);
    buttons[0].single_click(PointerEventButton::Left).await;
    slint::quit_event_loop().unwrap();
})
.unwrap();
slint::run_event_loop().unwrap();
```

What it offers:

- **Rust:** find elements by accessible label, element id or type name, or build an `ElementQuery`.
  Read accessibility fields and geometry, invoke accessibility actions and set values.
  Simulate `single_click`, `double_click`, `drag` and `scroll`.
- **Mock time:** `init_no_event_loop()` and `init_integration_test_with_mock_time()` mock the clock.
  `mock_elapsed_time()` advances animations and timers, so a test needs no waits.
- **C++:** `slint-testing.h` has the same queries, property reads and accessibility actions.
  It has no pointer simulation and no mock time.
- **Python and Node:** no in-process API.
  They have only the MCP server and system testing.

Limits:

- The crate is internal and doesn't follow semver.
  Pin it to the Slint version with `=x.y.z`; its README calls the API preliminary.
- By default, the testing backend renders no pixels and measures text with a fixed font size.
  Layouts that depend on text width can differ from the real application, unless `TestingBackendOptions` selects a renderer.
- The test creates the component itself, so code in `main()` doesn't run.
  Applications need their setup in a function the test can call.
- The test and the application share a process.
  A crash or a blocked event loop takes the test down with it.
  KDGuiTestMCP's README gives this as its reason for keeping the process boundary.

What this changes:

- For Rust applications, a test written from an MCP exploration can be an in-process test.
  It needs no launcher or port, runs in parallel, and can use mock time instead of waits.
- Out-of-process tests remain the only option for Python and Node.
  They're also the only way to test the real binary, including `main()`.
- Locators are useful in both modes.
  Written against a small element trait, one locator engine can run over the MCP server's JSON and over `ElementHandle`.

## The `slint-testing` Package

The visual editor's UI tests in [`tools/editor/ui-tests/`](tools/editor/ui-tests/) run on a Python package, `slint-testing` 0.6.1.
It isn't in this repository or on PyPI.
It comes from a private package index, `https://testing.slint.dev/simple/`, which needs a token.
CI fetches one, see [`.github/workflows/ci.yaml`](.github/workflows/ci.yaml).
No public repository holds its source.
It's a pure-Python wheel, so the wheel itself contains the source.
Reading it needs access from SixtyFPS, to the package index or to the repository.

It uses the system-testing transport.
Commit `1a7cb0ce4a` says so, where it makes that transport hold modifiers during clicks and drags.
The editor's tests agree:

- Its only dependency is `protobuf`.
- The tests call `window.dispatch_event()` 336 times with raw events, such as `slint_testing.PointerPressEvent`.
  System testing accepts raw window events; the MCP server has no tool for them.
- The gallery tests launch the gallery through `slint_testing.Application`.
  Their error message asks for a build with `--features system-testing` only, without `slint/mcp`.

Whether it also has an MCP client is unknown.
Python's standard library is enough for one, so the dependency list doesn't rule it out.

What the editor's tests use from it:

- `Application(args, env=..., launch_timeout=...)` launches the application and connects to it.
- `Window` and `Element` offer screenshots, accessibility actions, clicks and synthetic window events.
- `query_descendants()` filters with `match_id()`, `match_accessible_role()` and `match_accessible_label()`.
- `.tracking(timeout)` returns an element that runs its query again on every read or action.
  It follows the element when the UI rebuilds it.
- `wait_until()` retries with growing pauses of up to 200 ms.

The Playwright-style `expect(field).to_have_value("240")` in [`tools/editor/TESTING.md`](tools/editor/TESTING.md) isn't part of the package.
It's the editor's own `ui_assertions.py`, written on top of it.

### Releases and Direction

There's no public roadmap.
The editor's pinned versions and Slint's commits show where it's heading.

| Version | Pinned by the editor | What changed, from Slint's commit messages |
| --- | --- | --- |
| 0.3 | 2026-08-26 to 2026-09-30 | Returned a default value when the application reported an error |
| 0.4 | Never pinned | Tracking queries; `RequestError` on an application error; label matching in Python, one request per candidate |
| 0.5 | 2026-09-30 | `match_accessible_label()` evaluated in the application, with Slint 1.19 or later |
| 0.6.1 | 2026-10-02 | `wait_until()`; `absolute_rect`, which reads position and size from the same element instance |

- **Tracking queries replaced the editor's locators.**
  The editor had its own `Locator` classes, 272 lines in `ui_locators.py`.
  Commit `da6387d772` deleted them for `.tracking()`, which does the same job.
  SixtyFPS's answer to a stable locator is a query object in code, not a string an agent copies.
- **Slint changes for it land in this repository.**
  `match_accessible_label` moved label matching into the application, saving one request per candidate (`06f0451a97`).
  System testing now holds modifiers during clicks and drags (`1a7cb0ce4a`).
- **System testing is a committed public feature.**
  The Rust and C++ `system-testing` features "mirror the mcp feature and we should commit to them" (`69a39178d6`).
  The `slint-dev` Python wheel and the `slint-ui-dev` Node package include it.
- **Access is being worked on.**
  CI gives pull requests from forks a throwaway token "until the OIDC setup is ready".

tronical made all of the commits above.
The throwaway token comes from a gist of ogoffart's.

What this means:

- Launching, queries that follow a rebuilt UI, and waits already exist, in Python.
- It's moving fast, so its overlap with the planned test crate grows.
- The editor's tests use no locator strings, naming ladder, text snapshot or MCP front-end.
  We saw only how the tests use the package, not its source, so it may offer more.
- It's SixtyFPS's own product, so a KDAB tool could compete with it or build on it.
  Ask tronical and ogoffart what it's meant to become, and whether KDAB can use it, before building the same parts.

## Status of Issue #13244

[Issue #13244](https://github.com/slint-ui/slint/issues/13244) lists four gaps in verifying application state over MCP.

| Gap | Tracking | Status |
| --- | --- | --- |
| Modifiers on pointer tools | PR #13255 | Merged |
| Wheel and scroll events | PR #13253 | Merged |
| Reading declared properties | Issue #13251, PR #13372 | Open, review requested from tronical |
| Screenshot of one element | Issue #13252 | Open, blocked on #13250 |

Reading declared properties matters most here.
Without it there's no equivalent of KDGuiTestMCP's `waitUntil(gameCanvas, mode, arcade)` on a custom property.
Tests can only check accessibility values or compare pixels.

PR #13372 adds an opt-in `@testable` attribute:

- Marked properties become readable from generated Rust, generated C++ and the interpreter.
- They work on sub-components and repeated rows, not only the root component.
- `ElementHandle` gains `testable_properties()` and `testable_property_value()`.
- `get_element_properties` and `get_element_tree` return a `testableProperties` field with typed JSON values.
- It needs debug info, and it's read-only.
  Writing is planned as a follow-up, see [Setting Properties](#setting-properties).
- Only marked properties are listed, so the application's author decides what tests can read.
- It's experimental.
  Without `SLINT_ENABLE_EXPERIMENTAL_FEATURES=1`, the compiler rejects `@testable`.

[Generated Code](#generated-code) shows what the PR generates.

Screenshots of one element are a nice-to-have.
A text snapshot replaces most screenshots.

## Generated Code

To see where properties live at runtime, we generated Rust for a small sample with `slint-compiler -f rust`.
That's the same code generator `slint-build` uses.
The sample, [`gui-test-sample/app.slint`](gui-test-sample/app.slint), is about 40 lines.
It's a `Window` with a `Text`, a `Button`, a `LineEdit` and a `for` loop over a `Card` component.
[`gui-test-sample/app_testable.slint`](gui-test-sample/app_testable.slint) began as the same sample with four properties marked `@testable`.
It has since gained three more `@testable` properties and a `ListView` of todos, for the [compact snapshot](#changes-needed-in-slint).
The generated files below come from the earlier version.
We generated it on `master` and on the PR #13372 branch, each with and without `SLINT_EMIT_DEBUG_INFO=1`:

```sh
cargo build -p slint-compiler
./target/debug/slint-compiler -f rust gui-test-sample/app.slint -o /tmp/app.rs
SLINT_EMIT_DEBUG_INFO=1 ./target/debug/slint-compiler -f rust gui-test-sample/app.slint -o /tmp/app_debug.rs
# PR #13372 branch only
SLINT_ENABLE_EXPERIMENTAL_FEATURES=1 SLINT_EMIT_DEBUG_INFO=1 \
    ./target/debug/slint-compiler -f rust gui-test-sample/app_testable.slint -o /tmp/app_testable_debug.rs
rustfmt --edition 2024 /tmp/app*.rs
```

The compiler writes the whole file on one line, so `rustfmt` is what makes it readable.

The formatted output is in [`gui-test-sample/generated/`](gui-test-sample/generated/), about 24k lines per file:

| File | Compiler | Debug info |
| --- | --- | --- |
| [`master/app.rs`](gui-test-sample/generated/master/app.rs) | `master` at `f1ab413afa` | No |
| [`master/app_debug.rs`](gui-test-sample/generated/master/app_debug.rs) | `master` at `f1ab413afa` | Yes |
| [`pr-13372/app_testable.rs`](gui-test-sample/generated/pr-13372/app_testable.rs) | PR #13372 at `eeeadc9ade` | No |
| [`pr-13372/app_testable_debug.rs`](gui-test-sample/generated/pr-13372/app_testable_debug.rs) | PR #13372 at `eeeadc9ade` | Yes |

Search for `pub struct InnerApp` and `pub struct InnerCard_root` to find the structs below.
The PR branch is 52 commits behind `master`, so compare the two `pr-13372` files with each other, not with the `master` ones.

### What Lands in the Struct

Each component becomes a struct, with its elements and properties as fields:

```rust
pub struct InnerApp {
    root_69: sp::WindowItem,
    label_71: sp::SimpleText,                          // label := Text
    ok_button_72: InnerButton_root_3,                  // ok-button := Button
    input_73: InnerLineEdit_root_38,                   // input := LineEdit
    root_69_counter: sp::Property<i32>,
    root_69_status: sp::Property<sp::SharedString>,
    root_69_submitted: sp::Callback<(sp::SharedString,), ()>,
    repeater0: sp::Repeater<InnerComponent_card_74>,   // for name in names: Card
    // ...
}
```

- Built-in elements are fields, and components such as `Button` are nested structs.
- A `for` loop becomes a `Repeater`, and each row is its own instance of the row's struct.
- Each declared property is a `Property<T>` field, named `<element>_<number>_<property>`.
- An unused property has no field at all.
- Layouts such as `VerticalBox` dissolve into layout-cache properties.
- Debug info adds no property names.
  It adds a string per element, such as `"Button,App::ok-button,;,Button::root,"`.
  That's where the MCP tree gets its ids.
  It also keeps layouts in the tree as empty placeholder elements.

### Why Tests Can't Use It Directly

- The structs live in a private module, `slint_generatedApp`.
  Only the component handle `App` is exported, with accessors such as `get_counter()` for the root's public properties.
- The fields are private, and so are the field-offset accessors generated for them.
- Numeric suffixes such as `_69` change whenever the `.slint` file changes.
- The optimizer removes unused properties, so a field may not exist.

### What PR #13372 Generates

The PR adds two functions to every component.
They're generated inside the private module, so they can read the private fields.

- `element_testable_properties(index)` lists an element's `@testable` properties with their types, such as `"counter:int\nunused-flag:bool\n"`.
- `element_testable_property_value(index, name)` reads one field and formats it as a string.
- For an element inside a nested component or a repeated row, both functions pass the call on to that component.
  That's how Card's `selected` and private `click-count` become readable per row.
- Unmarked properties, such as `status` or `title`, aren't listed, even though their fields exist.

With debug info, a `@testable` property survives even when nothing reads it.
Without debug info, it's optimized away like any other unused property.
The two functions remain as empty stubs, 94 of them in the sample.
The PR measured 0.43% growth of the unstripped gallery binary, mostly from symbol names.

### How the MCP Server Reads Them

The generated functions reach an MCP client in five steps:

1. Two new `ItemTreeVTable` entries call the generated functions.
2. `ItemRc::element_testable_properties()` and `element_testable_property_value()` in [`internal/core/item_tree.rs`](internal/core/item_tree.rs) wrap them.
   A build without debug info reports "not available", which differs from "nothing marked".
3. `ElementHandle::testable_properties()` and `testable_property_value()` expose them, to in-process tests as well.
4. `element_properties()` in [`introspection/mod.rs`](internal/backends/testing/introspection/mod.rs) builds a list of name, type and value, plus an availability flag.
   Both transports share it, through protobuf fields 24 and 25.
5. `get_element_properties` and `get_element_tree` return the list as `testableProperties`.
   The MCP server turns each value into typed JSON by its type.
   Booleans become JSON booleans, and `int`, `float`, `length`, `duration`, `angle` and `percent` become numbers.
   Colors, enums and strings stay strings.

For one `Card` row of the sample, a reply would contain the following.
It's derived from the code, not from a run.

```json
"testableProperties": [
  {"name": "click-count", "typeName": "int", "value": 0},
  {"name": "selected", "typeName": "bool", "value": true}
]
```

What the PR doesn't add:

- **A query matcher.**
  A follow-up adds one, see item 2 of [Changes Needed in Slint](#changes-needed-in-slint).
  With it, a locator such as `prop:selected=true` takes one `query_element_descendants` call.
  Without it, the client filters `get_element_tree` output.
- **A tool for a single property.**
  A `waitUntil(locator, property, expected)` polls `get_element_properties` and picks the property from the list.

## Setting Properties

The in-app MCP server can't set a property yet.
Its only write is `set_element_value`, which sets the accessible value.
A `set_element_property` tool is planned, see [What This Means for the Tool](#what-this-means-for-the-tool).
Elsewhere, Slint sets properties in four ways.
Three different programs are involved:

- `slint-viewer` ([`tools/viewer/`](tools/viewer/)) runs a `.slint` file on its own.
- `slint-lsp` ([`tools/lsp/`](tools/lsp/)) is the language server.
  Its live preview is what editors such as VS Code show.
- `slint-editor` ([`tools/editor/`](tools/editor/)) is the standalone visual editor, with its own preview canvas.

| Mechanism | Where | What it reaches | Runtime |
| --- | --- | --- | --- |
| `--load-data file.json` | `slint-viewer`, at startup | The root component's public properties and global properties | Interpreter |
| Preview data | `slint-lsp`'s live preview | The root component's public properties and global properties | Interpreter |
| Debug-hook overrides | `slint-editor` | Any element's bound properties | Interpreter, compiled with debug hooks |
| Generated setters such as `App::set_counter()` | Compiled Rust and C++ | The root component's public properties | Compiled code |

The live preview's data panel calls the same `set_property()` and `set_global_property()` as `--load-data`.
See `set_preview_data()` in [`tools/lsp/preview/preview_data.rs`](tools/lsp/preview/preview_data.rs).

### `slint-viewer --load-data`

- The JSON keys are the root component's public properties, plus globals as `"Global.prop"` or `{"Global": {...}}`.
- The viewer converts each value with `Value::from_json()` and calls `ComponentInstance::set_property()` or `set_global_property()`.
- `--save-data` writes the root's public properties back out at exit.
- Setting by name works because the interpreter keeps the property table at runtime.
  The Python and Node APIs work the same way.

See `load_data()` in [`tools/viewer/main.rs`](tools/viewer/main.rs).

### Visual Editor Overrides

- `slint-editor` compiles the design it shows with debug hooks enabled.
- The `inject_debug_hooks` compiler pass then wraps every property binding in a hook.
  Each hook has an id built from the element and the property name.
- The editor installs a callback with `set_debug_hook_callback()`.
  It returns one override slot per id, and setting `Some(value)` replaces the binding's live value.
- That's how inline text editing and rotation dragging work without touching the source.
- It's internal API, and works only in the interpreter.
- Neither `slint-viewer` nor `slint-lsp`'s live preview installs these hooks.

See `install_debug_hook_callback()` in [`tools/editor/preview.rs`](tools/editor/preview.rs).

### What This Means for the Tool

- **Fixtures work today, in a different form for each target.**
  - `.slint` files in `slint-viewer`: the launcher passes `--load-data fixture.json`.
  - In-process Rust and C++ tests: the test calls the generated setters, such as `app.set_counter(3)`.
  - Compiled binaries launched in a separate process: only what the application itself accepts, such as arguments or environment variables.
- **A `set_element_property` MCP tool is planned** as a follow-up to PR #13372.
  The PR's description sketches it:
  - A third vtable function writes a `@testable` property, next to the two that read it.
  - An MCP tool, `set_element_property`, exposes it.
  - Setting a value replaces the property's binding.

  As of 2026-10-13 there's no issue or pull request for it yet.
  If it follows the read side, it covers compiled Rust, compiled C++ and the interpreter alike.
  In the interpreter alone, debug-hook overrides could already reach any element's properties,
  as `slint-editor` does.
  `slint-viewer` would have to compile with debug hooks enabled and expose them.

There's no date for `set_element_property`, so the tool doesn't wait for it.
It sets up state with fixtures and synthetic input, and gains a `set` tool once the Slint side lands.

## Driving Slint's Own Tools

An MCP client can drive two of the three programs from [Setting Properties](#setting-properties):

| Program | MCP | What a client can do |
| --- | --- | --- |
| `slint-viewer` | Its `mcp` feature | Drive the running design, whose `TouchArea`s receive the clicks |
| `slint-editor` | Built with `--features slint/mcp` in CI and [`TESTING.md`](tools/editor/TESTING.md) | Drive the editor's UI: outline, property panel, canvas |
| `slint-lsp`'s live preview | No `mcp` feature | Nothing, out of the box |

- **`slint-viewer`:** with `--auto-reload`, an agent edits the `.slint` file and inspects the reloaded design.
  After a reload, element handles are stale and must be looked up again.
- **`slint-editor`:** the editor writes edits made through its UI back to the `.slint` source.
  The design's elements are reachable too.
  The editor's UI tests look them up by id, such as `Main::root-rectangle`, through `slint-testing`.
  Whichever transport it uses, MCP shares the same element lookup.
- **`slint-lsp`:** it depends on `slint`, so a build with `--features slint/mcp` might work.
  We haven't tried it.
- In all three, an MCP client can't set a property directly.

We haven't run `slint-viewer` or `slint-editor` with MCP.
This section comes from their build setup and tests.

## Changes Needed in Slint

Each of these is small and useful on its own, so each can be a separate pull request.
Items 1, 2, 4 and 5 are done.
They're committed on top of the PR #13372 branch, but not yet in a pull request.
Below them sits a fix found along the way:
`ElementHandle::visit_descendants` now stops when the visitor breaks inside a popup attached to the element it starts from.

1. **Parent links in the tree.** Done in "testing/mcp: link each get_element_tree entry to its parent".
   `get_element_tree` returned a flat list without parent or depth.
   Without them, the client couldn't scope a locator to an ancestor or indent a snapshot.
   Each entry now has a `depth`, 0 for the requested element, and every other entry has a `parentHandle`.
   That's the handle of its nearest ancestor in the same response.
   A popup's contents link to the element the popup is attached to.
   That includes native popups, which the Qt backend opens in a window of their own.
   Children of clipped or off-screen elements link past them, since those aren't listed.
   Both this change and PR #13372 edit the node loop in `get_element_tree`, so it's based on that PR.
2. **More query matchers.** Done in "testing: match queries by accessible strings and @testable values".
   `ElementQuery` matched only exact id, type, base type, role or label.
   It now also matches accessible value, description, placeholder text and accessible id.
   `match_testable_property()` selects by a `@testable` value, so locators can select by state.
   Each matcher takes an exact string, or `TextMatch::ContainsIgnoringCase` for a case-insensitive substring.
   That's how KDGuiTestMCP's `text:` and `prop:key~value` locators match.
   MCP and system testing reach them through `matchElementAccessibleProperty` and `matchElementTestableProperty`.
   The MCP tool accepts `@testable` values typed as `get_element_properties` reports them, such as `true`.
   Accessible values stay strings, so a client passes a slider's value as `"42"`.
   It depends on PR #13372.
3. **A port without a race.**
   `SLINT_MCP_PORT` must be a fixed number.
   Accepting port 0 and reporting the bound port would let a launcher start applications safely.
   System testing doesn't have this problem, since the driver listens and the application connects.
4. **Scroll into view.** Done in "testing: scroll elements into view, creating ListView rows by paging".
   Queries listed only elements on screen, so an element below the fold of a `ScrollView` couldn't be found.
   `ListView` creates only the rows on screen, so an off-screen row didn't exist at all.
   `ElementQuery::scroll_into_view()` also searches content that a `Flickable` scrolled out of view.
   If that finds nothing, it pages through each `Flickable` at or below the query's root from the top, which creates the rows.
   It scrolls the first match into view and returns it.
   If paging finds nothing either, it puts every `Flickable` back where it was.
   `ElementHandle::scroll_into_view()` reveals an element the client already holds, such as a partly visible row.
   Both build on `ItemRc::try_scroll_into_visible()`, which Slint already uses to keep the focused element visible.
   MCP and system testing reach them through a `scrollIntoView` flag on `query_element_descendants` and a `scroll_element_into_view` request.
   Plain queries and `get_element_tree` still list only what's on screen.
   Paging takes time linear in the list's length.
   It recreates a `ListView`'s rows, so earlier handles to them become invalid.
5. **A compact snapshot.** Done in "testing/mcp: list the UI compactly with get_element_snapshot".
   `get_element_tree` returned every field of every element as JSON, which cost an agent many tokens.
   `get_element_snapshot` lists the same subtree as text, one line per element on screen with an accessible role or `@testable` properties.
   Without arguments, it lists every window, so an agent sees the UI in one call.
   It also lists input handlers such as `TouchArea`, unless an element with a role contains them, as in the std widgets.
   It leaves out a `Text` that repeats the label of the element it's listed under, such as the caption of a hand-made button.
   Each line shows the handle, type, role, id, label, value, states and `@testable` properties.
   A `TouchArea` with `enabled: false` shows as `disabled`.
   Lines are indented under the nearest listed ancestor.
   With `rectangles: true`, each line also ends in the element's rectangle.
   It's in window coordinates, also for an element in a popup, so it fits `move_pointer`.
   Action tools such as `click_element` take `snapshot: true` and then append a snapshot of every window.
   KDGuiTestMCP's front-end does the same after every action, but there it isn't optional.
   For the extended sample in a 400×500 window, the snapshot is 0.9 KB, or 1.3 KB with rectangles.
   `get_element_tree` returns 42 KB:

   ```text
   # 22 of 82 elements on screen. [i:g] is the element handle {"index": "i", "generation": "g"}.
   [1:1] Window id=App::root @counter=0 @last-submitted="" @todo-scroll=0.0 @unused-flag=false
     [3:1] Text id=App::label "Count: 0"
     [4:1] Button id=App::ok-button "Increment"
     [11:1] LineEdit role=TextInput id=App::input placeholder="Name"
     [20:1] Card @click-count=0 @selected=false
       [21:1] Text "a"
       [22:1] TouchArea
     ...
     [29:1] ListView role=List id=App::todo-list
       [32:1] TodoRow @done=false
         [33:1] CheckBox "Buy milk" unchecked
       [41:1] TodoRow @done=true
         [42:1] CheckBox "Walk the dog" checked
       ...
   ```

   The agent turns `[index:generation]` back into a JSON handle.
   Accepting the short form directly would save that step, but it changes every tool's input schema.

An idle check for running animations is optional.
KDGuiTestMCP has no settle detection either and relies on polling.

## Plan

Agents explore through the in-app MCP server directly.
Every other piece goes where it's used:

| Piece | Where | Notes |
| --- | --- | --- |
| Compact snapshot, parent links, query matchers, port 0, scroll into view | Slint, as five pull requests | See [Changes Needed in Slint](#changes-needed-in-slint); every language and every MCP client gets them; all but port 0 are done |
| Client for the in-app MCP over HTTP | Test crate | Small: JSON-RPC `tools/call` over HTTP POST; tests use it, agents don't |
| Launcher, session file, crash detection | Test crate | Simpler than Qt's: no agent lookup, `LD_LIBRARY_PATH` or display checks |
| Locators and naming ladder | Test crate | Naming signals listed below; outside Slint, so it works against released versions |
| Polling waits for a value, an element appearing, an element disappearing | Test crate | Same as KDGuiTestMCP |
| Key names such as Return, Tab and arrows | Test crate | Like KDGuiTestMCP's `keys.rs`: `dispatch_key_event` takes text |
| `gui_test!` harness | Test crate | Port almost directly |
| `launch` and `snapshot` commands | CLI on the test crate | Its `snapshot` adds a column of verified locators for tests to copy |
| Fixtures | Launcher | `slint-viewer --load-data` for `.slint` files; root properties and globals only, see [Setting Properties](#setting-properties) |

The agent's loop:

1. `guitest launch <target>` starts the application on the port that `.mcp.json` names.
   If Claude Code started before the application, reconnect once with `/mcp`.
2. The agent explores over MCP, reading the compact snapshot after each action.
   It passes `snapshot: true` to the action, so seeing the effect takes no extra call.
3. `guitest snapshot` prints verified locators for the elements the test needs.
4. The agent writes the test with those locators and runs it.
   It must fail for the reason the report gives.

Tests started by the harness use port 0 instead, so they can run in parallel.

### Why No MCP Front-End

KDGuiTestMCP's `guitest-mcp` sits between the editor's agent and the application.
Three of its reasons to exist don't carry over to Slint without recording:

- **Bridging the transport.**
  The Qt agent speaks JSON-RPC on a unix socket, not MCP.
  Slint's server already speaks MCP over HTTP, so `.mcp.json` points at it directly.
- **Resolving locators.**
  The Qt agent's ids are generated per run, such as `id#412`.
  Slint's handles stay valid for as long as the element lives.
  `find_elements_by_id` and `query_element_descendants` already find elements by id, accessible property or `@testable` value.
- **Turning a session into a test.**
  Its `waitUntil`, `assertProperty` and `script` tools record steps and emit a `gui_test!`.
  That's recording, which this evaluation excludes.
  The front-end's instructions call these checks "the step that makes the session worth keeping".

Its snapshot after every action is the one feature with value on its own.
Slint's server now has it, as `get_element_snapshot` and the `snapshot` argument on actions, so agents get it without installing anything.

A front-end would also cost:

- A second set of tools to keep in step with the in-app server.
  KDGuiTestMCP's history shows that work, in commits such as "Refuse an agent older than the client".
- Duplicate tools, if both servers are configured.

What a front-end would still add:

- **A connection that outlives the application.**
  [Claude Code](https://code.claude.com/docs/en/mcp) connects to an HTTP MCP server at startup, and marks it failed if the application isn't running.
  The user then reconnects with `/mcp`.
  A restart within about 30 seconds recovers on its own, but a rebuild can take longer.
- **Launching from MCP clients without a shell,** such as Claude Desktop.
- **Recording.**
  Only a process that sees every call can transcribe a session.

Revisit the front-end if recording comes into scope, or if reconnecting proves too disruptive in practice.

### Launch Targets and Naming Signals

The launcher targets:

- a binary built with cargo,
- a C++ binary,
- `slint-viewer file.slint`,
- a Python script.

Naming signals available in Slint:

- the qualified element id, such as `Form::submit`,
- the type name and its base types,
- accessibility label, role, value, description, placeholder and accessible id,
- `@testable` properties, once PR #13372 merges.

The locator grammar, naming ladder, snapshot renderer, waits and harness are mostly independent of Qt.
Copy and adapt them rather than rewriting them.
Decide whether to share code with KDGuiTestMCP once the Slint naming ladder has been measured.

The in-tree demos make a ready-made corpus for that measurement:
gallery, printerdemo, todo, 7guis, home-automation and energy-monitor.

## Optional Phase: Gherkin Tests

The agent writes the tests.
That removes Gherkin's main reason, which is letting people who don't program write tests.
So the default test format is code: a `gui_test!` in Rust, or pytest if the tool builds on `slint-testing`.
Start this phase only when a specific reader outside development needs feature files.

### When It's Worth It

Where Gherkin still pays off:

- **Sign-off by people who don't program.**
  The agent turns a prose bug report into a scenario.
  QA or product confirm that it describes the intended behavior, before or after the agent implements it.
  People review what the test checks, and the agent maintains the code.
- **Existing processes.**
  Some customers have Squish BDD suites, or rules that tie tests to requirements through feature files.
- **Portability.**
  Feature files survive a change of test tool, as long as the step wording matches.

What Gherkin no longer buys, once the agent writes the tests:

- **Easier writing.**
  An agent writes a short Rust or Python test as easily as a feature file.
- **Guardrails.**
  A fixed list of steps keeps the agent from sleeping or clicking raw coordinates.
  A small client API that offers only locator-based actions and waiting checks does the same.
  KDGuiTestMCP works that way.

What it costs:

- **Two files instead of one.**
  With steps in the application's own terms, the agent writes and maintains the feature file and the step code.
  With generic steps, the feature reads like a script, and that's no clearer than a short code test.
  For example, with an API that isn't designed yet:

  ```rust
  app.click("text:Increment")?;
  app.wait_value("id:App::label", "text", "Count: 1")?;
  ```

- **Weaker computing.**
  Checks that have to calculate something fit poorly.
  KDGuiTestMCP's README singles those out as the tests that needed code.
- **Harder debugging.**
  A failure goes through regular-expression step matching, so fixing it means tracing across two files.

### How It Would Work

Neither KDGuiTestMCP nor Slint supports Gherkin feature files.
They fit on top of the tool without changes to Slint, so the phase can wait until it's needed.

A Gherkin step needs two things the tool provides anyway:

- **A way to name an element in plain text.**
  That's what locator strings are.
  With the session-only handles Slint's server uses today, steps would be impractical.
- **Checks that wait instead of sleeping.**
  Every `Then` step polls until it holds or times out, like KDGuiTestMCP's `waitUntil`.

The tests talk to the application over a socket, so the test language doesn't have to match the application's.
A C++ application can be tested from a Rust or Python runner.

| Runner | Fits when |
| --- | --- |
| Rust: the `cucumber` crate, in a test target with `harness = false` | The tool is a standalone Rust crate, and steps call its client library |
| Python: `pytest-bdd` or `behave` | The tool builds on `slint-testing`, whose editor tests already run on pytest |

An example with generic steps, against the sample.
The locator syntax isn't designed yet.

```gherkin
Feature: Counter
  Scenario: Clicking Increment raises the count
    Given the application "gui-test-sample/app.slint" is running
    When I click "text:Increment"
    Then "id:App::label" shows "Count: 1"
    And the testable property "counter" of the root is 1
```

What it takes, about 2–3 days on top of the Rust client or `slint-testing`:

- **A shared state object per scenario,** a "World" in Cucumber terms.
  It holds the connection to the running application, started per scenario or reused per feature.
- **A library of generic steps:**
  starting the application, optionally with a fixture through `slint-viewer --load-data`;
  clicking, typing, pressing keys, scrolling and hovering;
  checking visibility, text, accessible values and `@testable` properties.
- **Failure output:** the text snapshot and a screenshot, attached to the failed scenario.
- **An agent workflow:** the agent turns a bug report into a scenario, and a reader outside development confirms it.
  The agent then explores over MCP, and writes any missing steps from the locators `guitest snapshot` prints.

Caveats:

- **Generic steps read like scripts.**
  "Click X, type Y" is considered a Gherkin anti-pattern.
  The readers this phase is for need steps in the application's own terms, such as "When I add the todo 'milk'".
  Generic steps are only a starting point.
- **Property checks depend on PR #13372.**
  Until it merges, `Then` steps can check only accessibility fields, text and visibility.
- **Run them out of process.**
  The `cucumber` crate runs scenarios on its own async runtime.
  That doesn't mix easily with Slint's event loop and its single-threaded objects.

### Squish's BDD Mode

Squish, the commercial GUI test tool from Qt Group, runs Gherkin feature files.
KDAB's customers may already know it, so it's the model to compare against.
Its [Behavior Driven Testing](https://doc.qt.io/squish/behavior-driven-testing.html) chapter describes it.

A BDD test case has three parts:

- `test.feature` holds the Gherkin, including `Scenario Outline`, `Examples`, `Background` and tables.
  The `From` keyword loads a table from a `.csv`, `.tsv`, `.txt` or `.xls` file.
- `test.py` is generated next to it and only runs the feature.
  Squish also supports JavaScript, Perl, Ruby and Tcl.
- Step definitions live in `steps` folders.
  The test case's own folder comes before the suite's shared one, so a local step overrides a shared step with the same text.

```python
def main():
    testSettings.throwOnFailure = True
    setupHooks('../shared/scripts/bdd_hooks.py')
    collectStepDefinitions('./steps', '../shared/steps')
    runFeatureFile('test.feature')
```

Steps are decorated functions:

```python
@Step("I add a new person '|word|','|word|','|any|','|any|' to address book")
def step(context, forename, surname, email, phone):
    ...
```

- `@Given`, `@When` and `@Then` match their own keyword, or `And` and `But` after it.
  `@Step` matches any keyword.
- The placeholders are `|word|`, `|integer|` and `|any|`, or a regular expression with `regexp=True`.
- `context` carries `table`, `multiLineText` and `userData`.
  `userData` persists across steps, to share state.
- Hooks run on feature, scenario and step start and end.
  The documentation's example starts the application in `OnScenarioStart` and detaches in `OnScenarioEnd`.
- Step bodies are ordinary Squish code, such as `startApplication()`, `waitForObject(names.…)` and `test.compare()`.
  Objects are named through Squish's object map.

The Squish IDE adds three things:

- "Create Missing Step Implementations" writes placeholder steps for undefined ones.
- Recording plays a scenario up to its first missing step, then records the user's input as that step's code.
- After a run, it colors the feature file by result and logs each step.

Compared with our optional phase:

- **The model maps directly.**
  `context.userData` is the per-scenario World, the scenario hooks start and stop the application, and steps call the driver.
- **Recording missing steps is recording, which this evaluation excludes.**
  An agent working over MCP takes its place: it explores, then writes the feature file and any missing steps.
- **Python runners would feel familiar.**
  `pytest-bdd` and `behave` also use Python step files, decorators, placeholders and hooks.
  Squish feature files carry over only if our steps match their wording.
- **Generic steps are an addition.**
  The Squish documentation shows only steps written or recorded for each application.
- We didn't check whether Squish can drive Slint applications.

See also the [Java BDD tutorial](https://doc.qt.io/squish/squish-for-java-bdd-tutorials.html) and [sharing step definitions across test suites](https://qatools.knowledgebase.qt.io/squish/howto/sharing-step-definitions-across-test-suites-bdd/).

## Where Slint Is Easier

- **No injection.**
  There's no plugin to find, no fixed library name, and no need to match the toolkit's patch release.
- **MCP built in.**
  The application's server speaks MCP itself, so agents connect to it without a front-end.
- **Parallel tests.**
  With the headless backend, tests don't share a display, so they don't need a global lock or an Xvfb container.
- **Cross-platform.**
  The transport is localhost TCP, and liveness can use the child process handle instead of `/proc`.
- **Rows are elements.**
  List rows are ordinary elements, so there's no separate model-index layer.

## Where Slint Is Harder

- **The application has to be rebuilt.**
  Slint has no runtime plugin hook, so the tested binary needs the `mcp` feature and debug info.
  Python, Node and `slint-viewer` avoid this through the interpreter.
- **Ids in repeated components aren't unique.**
  `Card::title` matches every card.
  The naming ladder has to lean on ancestor scope, the accessible label or the item index.
  It's the same problem as a per-row `objectName` in a QML delegate.
- **Reading properties is opt-in.**
  Qt can read any `Q_PROPERTY` of any object.
  In Slint, a test can read only properties the author marked `@testable`, and only with debug info.
  Until PR #13372 merges, checks are limited to what the accessibility layer exposes.
- **`ListView` is virtualized.**
  An off-screen row doesn't exist, so finding it means paging through the list.
  The scroll-into-view change does that, in time linear in the list's length.
  Qt's model-index layer reads any row directly.

## Estimate

| Phase | Content | Time |
| --- | --- | --- |
| First version | HTTP client, launcher with fixtures, actions and waits, first naming ladder, CLI with `launch` and `snapshot` | ~1 week |
| Parity without recording | Harness, the remaining Slint pull request, naming ladder measured over the demos | +1–2 weeks |
| In-process mode | Element trait over `ElementHandle`, Rust harness on mock time | +2–3 days |
| Gherkin tests (optional) | Only when a reader outside development needs feature files, see [Optional Phase: Gherkin Tests](#optional-phase-gherkin-tests) | +2–3 days |

For calibration, KDGuiTestMCP took 224 commits from 2026-09-17 to 2026-10-05 for about 20k lines.
That included the agent, recording and coverage tooling.
Naming elements well is the open-ended part here, as it was there.

If the tool can build on `slint-testing`, launching, queries and waits already exist.
The first version then shrinks to locators, naming and the CLI.

## Recommendation

- Ask SixtyFPS about `slint-testing` first.
  Its answer decides whether the tool builds on it or stands alone.
- Let agents explore through the in-app MCP server directly, without an MCP front-end.
  See [Why No MCP Front-End](#why-no-mcp-front-end).
- Build the outside part as a test crate with a small CLI.
  The crate talks to the in-app MCP server, whose single-call tree suits snapshots and naming.
  If the tool builds on `slint-testing` instead, add a tree request to system testing, the transport it uses.
- Keep the locator logic in that crate, so it works against released Slint versions.
  Write it against a small element trait.
  One implementation talks to the MCP server; a second wraps `ElementHandle` for in-process Rust tests.
- Send only the five generic changes to Slint, including the compact snapshot that agents explore with.
  Parent links, query matchers, scroll into view and the compact snapshot are done.
- Set up state with fixtures and synthetic input.
  `set_element_property` is planned for later, with no date, so don't wait for it.
- Have the agent write code tests: a `gui_test!` in Rust, or pytest on `slint-testing`.
  Keep the test API small, with only locator-based actions and waiting checks.
  Add Gherkin only when a reader outside development needs feature files.
- Coordinate with tilladam, who wrote #13244 and PR #13372 and is working on the in-app side.

## Open Questions

- What is `slint-testing` meant to become, and can KDAB use it or build on it?
- Will SixtyFPS publish `slint-testing`, and under which license?
- Does `slint-testing` have an MCP client, or plans for one?
- Should the tool live in the Slint repository, under `tools/`, or as a separate project like KDGuiTestMCP?
- Should assertion design wait for PR #13372, or start with accessibility fields only?
- Does the harness need Python or C++ bindings beyond Rust?
- Should tests for Rust applications run in-process, out-of-process, or either?
- When a check needs a property that isn't listed, should the tool suggest adding `@testable` to it?
- Is reconnecting with `/mcp` after a launch acceptable?
  If not, the CLI could offer actions too, so agents can explore without an MCP connection.
- Do KDAB's users drive agents from MCP clients without a shell?
  Those can't launch an application without an MCP front-end.
- Does anyone outside development need to read or approve the tests?
  If so, or if KDAB's customers already use Gherkin, for example in [Squish's BDD mode](#squishs-bdd-mode), the Gherkin phase is worth it.
