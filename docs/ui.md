# Codex Relay UI Specification

## 1. Product Positioning

Codex Relay is a local desktop companion for Codex.

It is not:
- an IDE
- a chat app
- a dashboard
- a workflow builder
- an AgentOS
- a replacement for Codex

Codex remains the lead orchestrator.

Relay exists to make Codex delegation to external CLI / harness workers visible and controllable.

The interface should feel like a natural Codex companion: quiet, compact, developer-focused, and operational.

---

## 2. Core Product Principle

The UI has four layers:

```text
Sessions
= Which Codex session am I observing?

Step
= Which top-level Relay worker is active or selected?

Console
= What has that selected CLI recently done?

CLI Info
= What configuration and result metadata do I need about that CLI?
```

Everything else should be contextual, secondary, or drill-down.

---

## 3. Main Window

The normal workspace consists of:

- collapsible left sidebar
- main session area
- horizontal Step strip
- large Console area
- optional collapsible CLI Info inspector

Conceptually:

```text
┌──────────────────┬─────────────────────────────────────────────┐
│ Relay             │ Current Codex Session                      │
│                   │                                             │
│ Sessions          │ Step                                        │
│                   │                                             │
│ session A         │ [node] ── [node] ── [node]                │
│ session B         │                                             │
│ session C         ├───────────────────────────────┬─────────────┤
│ ...               │ Console                       │ CLI Info    │
│                   │                               │             │
│ Settings          │                               │             │
└──────────────────┴───────────────────────────────┴─────────────┘
```

Do not add a dashboard homepage.

---

## 4. Sidebar

### 4.1 Purpose

The sidebar should behave very similarly to the Codex desktop sidebar.

Its primary purpose is Codex session navigation.

### 4.2 Structure

Expanded:

```text
Relay

Sessions

Refactor worker runtime
Running · 12m

Fix adapter tests
09:42

Add resume support
18:20

Investigate memory leak
Sep 23

...

Settings
```

### 4.3 Session Rules

- newest / most recently active session is always first
- if an old session becomes active again, move it to the top
- do not group by Today / Yesterday / Previous 7 Days
- do not add icons before every session
- do not add a search field
- do not add a New Session button
- do not add Home / Dashboard / Agents navigation to the normal workspace sidebar
- keep session rows text-first and compact
- selected session uses a subtle selected background

### 4.4 Settings Placement

`Settings` lives in the sidebar, near the bottom.

It may use a small gear icon.

Do not put Settings in the global toolbar.

---

## 5. Sidebar Collapse / Focus Mode

The sidebar can be fully hidden, similar to Codex.

Expanded:

```text
Sidebar | Step
        | Console
        | CLI Info
```

Collapsed:

```text
Step
Console
CLI Info (optional)
```

Important behavior:

- hiding the sidebar hides all session rows
- do not leave behind a session icon rail
- main content expands to use the space
- remember the user's last expanded/collapsed state
- provide a compact sidebar toggle near the upper-left window chrome
- keyboard shortcut support is desirable

Focus mode should make the app primarily feel like:

```text
Step
────────────
Console
```

---

## 6. Session Header

The main content area identifies the selected Codex session.

Example:

```text
Refactor worker runtime
Running · Started 10:12
```

Keep it restrained.

Do not add:
- hero layouts
- KPI cards
- large status banners
- global Stop button
- search
- decorative metadata panels

Session-level actions may live in a subtle `···` menu.

Possible actions:
- Stop all workers
- Open workspace
- Copy session ID

---

## 7. Step

### 7.1 Name

The component is called:

**Step**

Do not call it Progress.

Progress implies percentage completion, which Relay usually cannot know reliably.

### 7.2 Purpose

Step is a horizontal current-position navigator showing only the top-level workers that Codex dispatched through Relay.

It has two jobs:

1. show top-level execution state
2. select which CLI is displayed in Console and CLI Info

### 7.3 Scope

Step represents only:

```text
Codex
  ↓
Relay
  ↓
top-level worker
```

Examples:
- Gemini Research
- DeepSeek Code
- Kimi Code
- GLM Code
- Grok Code
- Terra Code

Do not show CLI-internal child agents as Step nodes.

If DeepSeek Harness internally launches six agents, Step still shows only:

```text
DeepSeek Code
```

Runtime-internal orchestration belongs inside the CLI itself, not Relay's top-level visualization.

### 7.4 Orientation

Step is always horizontal.

Reason:
- the user primarily cares about the current node
- vertical layouts can push the active node below the fold
- current execution must remain immediately visible

If there are many steps:
- horizontally scroll
- auto-scroll active workers into view
- compress distant history when needed

Current-first is more important than graph completeness.

---

## 8. Step Node Design

Each node should visually combine:

- a lightweight workflow node
- a clickable tab/card

A line with plain text is not enough because users must understand what is clickable.

Example:

```text
┌────────────────────┐       ┌────────────────────┐
│ ✓ Gemini Research  │ ───── │ ● Kimi Code        │
│ Done · 2m14s       │       │ Running · 3m28s    │
└────────────────────┘       └────────────────────┘
```

Each node may show:
- provider icon
- profile / worker name
- status
- elapsed time

Selected node:
- slightly stronger border
- subtle selected background
- restrained accent
- no glow

Do not use oversized cards.

---

## 9. Provider Icons

Use recognizable provider/runtime icons for:

- DeepSeek
- GLM
- Gemini
- Grok
- Kimi

These icons are appropriate in:
- Step nodes
- Settings provider list
- CLI Info when useful

Do not use provider icons for Codex session rows.

Keep provider branding restrained.

Status is more important than brand color.

---

## 10. Parallel Execution

Parallel execution means multiple **top-level Relay workers** were dispatched concurrently by Codex.

Example:

```text
✓ Gemini Research ── [ ● Kimi Code   ● DeepSeek Tests ] ── ○ Review
```

Rules:
- remain horizontal
- 2–3 parallel workers can appear side-by-side in one lightweight group
- do not draw a large DAG
- do not turn Relay into a workflow editor
- do not display runtime-internal child agents here

Example:

```text
┌ Parallel · 2 running ──────────────────────┐
│  ● Kimi Code          ● DeepSeek Tests     │
│  Running · 3m28s      Running · 1m12s      │
└─────────────────────────────────────────────┘
```

If many top-level workers are running:

```text
[ Parallel · 5 running ]
```

and reveal details on click/hover.

---

## 11. Step Selection

Clicking a Step node selects that CLI.

Selection must immediately control both:

```text
Console
CLI Info
```

Example:

```text
Step:
Kimi Code selected

↓

Console · Kimi Code
CLI Info · Kimi Code
```

Selecting DeepSeek must immediately switch both lower regions to DeepSeek.

This linkage must be visually obvious.

---

## 12. Console

### 12.1 Purpose

Console is the largest content area.

It shows a concise, structured execution summary for the selected CLI.

Console is not the full raw CLI output.

Console is not a reasoning viewer.

It answers:

> What has this worker recently done?

### 12.2 Event Examples

```text
10:14  Read      src/runtime/worker.ts
10:15  Search    resumeSession
10:16  Edit      src/adapters/kimi.ts       +42 -12
10:17  Command   pnpm test --filter runtime
10:18  Test      42 passed
10:19  Edit      src/runtime/session.ts      +18 -4
10:23  Result    Build completed
```

Useful categories:
- Read
- Search
- Edit
- Create
- Delete
- Command
- Test
- Build
- Result
- Error
- Warning
- Install
- Download
- Commit, when supported

### 12.3 Aggregation

Repetitive low-value events may be grouped.

Example:

```text
Read      12 files
Search    worker lifecycle
Edit      3 files
Test      42 passed
```

Do not hide important actions.

### 12.4 No Hidden Reasoning

Do not display hidden chain-of-thought or internal model reasoning.

Console may show:
- file operations
- searches
- tool calls
- shell commands
- tests
- stdout/stderr summaries
- errors
- public CLI/runtime messages

---

## 13. Console Header

Example:

```text
Console · Kimi Code            Running · 3m28s     Raw Output   Stop
```

Optional lightweight metadata:

```text
4 agents
```

Only show an internal-agent count if the CLI exposes it naturally.

Relay does not need to know:
- child-agent names
- child-agent tasks
- child-agent timelines
- child-agent statuses

The count is informational only.

---

## 14. Raw Output

Raw Output is a drill-down surface.

It contains the complete observable CLI output captured by Relay, including where available:

- stdout
- stderr
- tool output
- command output
- error stacks
- runtime messages
- structured observable events

Raw Output does **not** mean hidden model reasoning.

Do not permanently occupy the main UI with Raw Output.

Open it as:
- sheet
- temporary inspector
- secondary detail panel
- dedicated drill-down view

Do not create a permanent tab strip such as:

```text
Overview | Changes | Subagents | Raw Output
```

---

## 15. CLI Info

CLI Info is contextual metadata for the currently selected Step.

It should be a right-side inspector and should be collapsible.

Example:

```text
Kimi Code
Running · 3m28s

Runtime
Kimi Code

Model
kimi-k2

Reasoning
High

Working Dir
/workspace

Started
10:14:32

Agents
4

3 files modified
```

CLI Info always follows Step selection.

It is not an independent panel with its own unrelated selection state.

---

## 16. Model and Reasoning Configuration

### 16.1 Default Configuration

Long-term defaults belong in Settings.

Path:

```text
Settings
→ Agents
→ DeepSeek / GLM / Gemini / Grok / Kimi
```

Each provider/profile may configure:

- Enabled
- Runtime
- Profile name
- Model
- Reasoning strength
- Permissions
- CLI path
- runtime-specific parameters

Example:

```text
DeepSeek

Enabled

Runtime
DeepSeek Harness

Profile
DeepSeek Code

Model
DeepSeek Pro

Reasoning
High

Permissions
Read / Write / Shell
```

Possible model variants may include things such as:
- Flash
- Pro

Reasoning may include:
- Low
- Medium
- High

Do not put model/reasoning selectors inside Step nodes.

### 16.2 Session Overrides

CLI Info may optionally allow temporary session-level overrides:

```text
Model
Reasoning
```

Hierarchy:

```text
Settings default
    ↓
Current session override
    ↓
Actual worker launch
```

Example:

```text
Default:
DeepSeek Code = Pro / High

Current session:
DeepSeek Code = Flash / Medium
```

Do not change model/reasoning in the middle of an already running worker.

If changed while active, indicate that it applies to the next run / next resume where supported.

---

## 17. Settings

### 17.1 Opening Behavior

Clicking Settings should **not** replace the whole main window with a full-screen settings page.

Instead, show a compact centered Settings card/panel inside the application window.

It should feel similar to a focused macOS preferences surface.

Characteristics:
- constrained width
- constrained height
- centered in the current window
- visually separate from the workspace
- internal scrolling when necessary

Do not stretch Settings edge-to-edge.

### 17.2 Settings Navigation

Inside the Settings card, use a left-side option list.

Example:

```text
General

Agents
  DeepSeek
  GLM
  Gemini
  Grok
  Kimi

Workspace

Appearance

Advanced
```

Use provider icons beside:
- DeepSeek
- GLM
- Gemini
- Grok
- Kimi

The selected settings item renders its controls on the right side of the same centered card.

### 17.3 Settings Style

Use:
- light gray / white surfaces
- subtle blue selection
- compact controls
- native-feeling dropdowns
- switches
- segmented controls when appropriate
- small rounded rectangles
- subtle separators

Avoid:
- full-screen settings
- admin-dashboard layout
- huge cards
- giant headings
- gradients
- neon
- AI purple
- excessive empty space

---

## 18. Changes

Changes are useful but are not a permanent top-level tab.

CLI Info may show:

```text
3 files modified
```

Clicking it opens a deeper view:

```text
M  src/adapters/kimi.ts      +42 -12
M  src/runtime/session.ts    +18  -4
A  src/runtime/worker.ts     +96
```

Clicking a file may open a diff.

Do not create a generic `Files` page.

The distinction is:

```text
Console
= execution summary

Changes
= concrete file result

Raw Output
= complete observable CLI log
```

---

## 19. Stop Behavior

Stop is contextual.

Worker-level Stop belongs beside the currently selected CLI.

Example:

```text
Console · Kimi Code                  Raw Output   Stop
```

This means:

```text
Stop Kimi Code
```

Do not place a permanent global Stop button in the window toolbar.

If session-level Stop All is supported, place it in the session contextual menu:

```text
···
Stop all workers
```

Do not expose a permanent bright-red Stop All button.

---

## 20. Visual Direction

The visual language should be close to the Codex desktop app.

Primary characteristics:
- light theme first
- neutral white / gray canvas
- black and gray typography
- restrained blue accent
- thin dividers
- compact information density
- small corner radii
- minimal shadows
- very limited icon usage
- quiet hierarchy
- desktop developer utility feel

The app should look like:

> a native Subagent Monitor that could plausibly have shipped inside Codex.

Avoid:
- dark cyberpunk visuals
- neon
- gradients
- glow
- glassmorphism
- purple AI branding
- giant SaaS cards
- hero sections
- KPI tiles
- dashboard grids
- excessive pill badges
- decorative AI illustrations

---

## 21. App Icon

The app icon should remain provider-neutral.

Do not use:
- the letter C
- Codex-specific marks
- provider initials

The icon should represent:
- relay
- routing
- branching
- forwarding
- handoff

Use a simple geometric branching / relay symbol.

The icon must still make sense if the product name changes from:

```text
Codex Relay
```

to:

```text
Relay
```

Visual direction:
- simple
- geometric
- restrained
- tool-like
- compatible with Codex
- neutral color treatment
- not cyberpunk

---

## 22. Explicit Non-Goals

Do not add these unless explicitly requested later:

- global search bar
- Today / Yesterday grouping
- session icons
- Dashboard
- Home page
- New Session button
- top-level analytics
- topology graph
- node editor
- workflow builder
- DAG canvas
- vertical Step timeline
- fake progress percentages
- percent progress bars
- permanent global Stop button
- Overview / Changes / Subagents / Raw Output tab strip
- runtime child-agent tree
- hidden model reasoning
- full-screen Settings page
- dark cyberpunk styling
- giant cards
- SaaS dashboard layout

---

## 23. Canonical Workspace

```text
┌──────────────────────┬────────────────────────────────────────────────────┐
│ Relay                │ Refactor worker runtime                           │
│                      │ Running · Started 10:12                            │
│ Sessions             │                                                    │
│                      │ Step                                               │
│ Refactor worker...   │                                                    │
│ Running · 12m        │ [✓ Gemini Research] ─ [● Kimi Code] ─ [○ Review] │
│                      │                                                    │
│ Fix adapter tests    ├──────────────────────────────────┬─────────────────┤
│ 09:42                │ Console · Kimi Code              │ CLI Info        │
│                      │ Running · 3m28s   Raw Output Stop │                 │
│ Add resume support   │                                   │ Kimi Code       │
│ 18:20                │ 10:14 Read    worker.ts          │ Running         │
│                      │ 10:15 Search  resumeSession      │ Runtime         │
│ Investigate memory.. │ 10:16 Edit    kimi.ts +42 -12   │ Model           │
│ Sep 23               │ 10:17 Command pnpm test         │ Reasoning       │
│                      │ 10:18 Test    42 passed          │ Working Dir     │
│ ...                  │ 10:23 Result  Build completed    │ 4 agents        │
│                      │                                   │ 3 files modified│
│ Settings             │                                   │                 │
└──────────────────────┴──────────────────────────────────┴─────────────────┘
```

---

## 24. Canonical Focus Mode

When the sidebar is hidden:

```text
Refactor worker runtime

Step

[✓ Gemini Research] ─ [● Kimi Code] ─ [○ Review]

────────────────────────────────────────

Console · Kimi Code                     Raw Output   Stop

10:14 Read      worker.ts
10:15 Search    resumeSession
10:16 Edit      kimi.ts
10:17 Command   pnpm test
10:18 Test      42 passed
...
```

The workspace should feel focused almost entirely on:

```text
Step + Console
```

---

## 25. Design Summary

Relay should visually and behaviorally feel close to Codex.

The product should not invent a second AI workspace.

Codex remains where users think, plan, and interact.

Relay is the quiet operational companion that shows:

```text
who Codex delegated to
where execution is now
what the selected CLI is doing
how to inspect or control that worker
```
