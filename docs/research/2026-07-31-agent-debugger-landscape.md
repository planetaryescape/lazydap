# The agent-debugger landscape, and an adversarial audit of lazydap's claims

**Date:** 2026-07-31 · **Method:** three parallel research agents (as-built capability
inventory from code; competitive web sweep; adversarial refutation brief), synthesised by the
orchestrating session, with the decisive disputed fact verified against the primary source.
This document is the record; positioning copy derives from it, not the other way around.

## The verdict, in brief

The claim under audit: *"lazydap lets an agent speak with a real debugger directly and
ergonomically, in a way it couldn't before."*

- **"In a way it couldn't before" is false — retired.** At least three independent CLIs
  shipped the daemon+CLI shape during 2026 before lazydap's v0.1.0:
  [debug-skill](https://github.com/AlmogBaku/debug-skill) (Mar 2026, ~308★, MIT — blocking
  execution commands returning location, source, locals, stack and program output; `--json`;
  invisible auto-daemon; debugpy/dlv/js-debug/lldb-dap),
  [debug-that](https://github.com/theodo-group/debug-that) (Feb 2026, ~158★, Theodo-backed,
  CDP+DAP, ships a Claude Code skill),
  [debugger-cli](https://github.com/akiselev/debugger-cli) (Jan 2026, GPL-3.0, two-call
  await model), and [dapi](https://github.com/shmulc8/dapi) — whose author's essay,
  ["Give Your Agent a Breakpoint"](https://shmulc.substack.com/p/give-your-agent-a-breakpoint),
  documents the convergence: "several developers independently arrived at the same insight."
  lazydap is a member of a convergence cluster, not its origin.
- **Against the incumbents, the claim stands.** gdb `--batch` is stateless and MI demands an
  event loop ([output syntax](https://sourceware.org/gdb/current/onlinedocs/gdb.html/GDB_002fMI-Output-Syntax.html));
  lldb has no JSON CLI and its first-party MCP ships an async footgun with no program output
  ([lldb MCP](https://lldb.llvm.org/use/mcp.html)); Delve's JSON-RPC cannot return program
  output at all and the go-delve org built
  [an MCP mediator](https://github.com/go-delve/mcp-dap-server) rather than pointing agents
  at it; pdb is prompt-scraping. The de-facto incumbent for agents is
  `tmux send-keys + sleep + capture-pane`.
- **Against the agent-tool field, the claim survives narrowed.** The flagship MCP debugger
  documents fire-and-forget `continue` + polling
  ([mcp-debugger](https://github.com/debugmcp/mcp-debugger); its issue #214: "no way to
  detect a crash"). Everything with real traction — VS 2026's `@debugger`, JetBrains Junie,
  [microsoft/DebugMCP](https://github.com/microsoft/DebugMCP) (~443★), CLion 2026.2's
  skill — requires a live IDE. Cloud sandbox agents structurally cannot hold sessions.
  The headless lane is real, open, and small.

## What survives the audit as ownable (each has a named opposite)

1. **The contract, not the capability.** A documented `--wait` state machine
   (`paused|exited|terminated|timeout|adapter_died`, `raw_reason`, `hit_breakpoint_ids`,
   `breakpoint_updates`, `additional_stopped_threads`, `output_truncated`), a stable-JSON
   schema promise ("don't break it without a decision-log entry"), and a documented exit-code
   contract. The cluster has flags where lazydap has promises: debug-skill's `--json` has no
   schema commitment, no exit-code contract, no piping examples; debug-that deliberately bets
   the opposite (compact `@ref` text over machine-parseable output).
2. **The CLI is the product; the skill wraps it.** debug-skill positions the skill as primary
   and its CLI as "supporting infrastructure." lazydap's binary serves the agent, the shell
   script, the CI job, and the TUI identically — `jsonl`/`csv`/`ids` formats, `ids | xargs`
   composability, structured errors on stderr, tty autodetection, `--dry-run`. Claim the
   documented contract, not the rival's incidental behaviour.
3. **Output inside the state blob** — the least-copied property in the field (absent from
   Delve's RPC, lldb-mcp, and mcp-debugger's tools).
4. **The absorbed-async engineering as tested fact**: watermark delivery, stopped-coalescing,
   backlog filtered by meaning, execution permit held across the whole run, debuggee reaping
   with pid-identity checks, visible quirk normalisation (`reason`/`raw_reason`).
   [Debug2Fix](https://arxiv.org/html/2602.18571v2)'s key finding — raw debugger tools
   produced negligible gains; *mediated, context-bundling interfaces* won — is the empirical
   endorsement of exactly this layer.

## The dangers, ranked

1. **debug-skill** — same pitch, earlier, four adapter families to lazydap's two, roughly
   double the visible traction. Compete on the contract, or leapfrog on adapters (delve and
   js-debug are already on lazydap's roadmap).
2. **Category risk** — most successful agent coding today happens without dynamic debugging
   (the dapi author's own concession; Cursor built
   [Debug Mode](https://cursor.com/blog/debug-mode) on that counter-bet). Best available
   counter-evidence: [debug-gym](https://arxiv.org/abs/2503.21557) (+15pts on SWE-bench
   Lite with debugger access), Debug2Fix (+10–12pts; a cheap model with a debugger beat an
   expensive one without).
3. **A vendor going headless** — Microsoft owns the full stack (research + DebugMCP + VS
   2026) but everything is IDE-tethered today; JetBrains ships real agent debugging but
   IDE-required; if Cursor ever flips from log-instrumentation to DAP it leads instantly.

## Copy consequences applied (2026-07-31)

- Every "first"/"only"/"couldn't before" framing removed; the defensible form is
  *"the specified, tested version of the shape the ecosystem is converging on."*
- `why-lazydap.md`'s gap section names the convergence cluster and differentiates on
  contract; a sixth trade-off (CLI-as-product vs skill-first) added.
- README/llms.txt staleness fixed (debugpy, watches, REPL had shipped past the copy).
- GDB/MI data-model lineage conceded where relevant: stop-reason/frame/target-output have
  been machine records since ~2000; the packaging — transport, framing, contract — is the
  product.

---

*The three full reports follow as appendices: A. as-built inventory (code ground truth),
B. competitive sweep, C. refutation brief. They are preserved verbatim as delivered by the
research agents; star counts and version claims are as-of 2026-07-31. Appendix C contains
one known error: it describes lazydap as "pre-alpha, M2-2 done" from stale public docs —
v0.1.0 had shipped with two adapters at audit time.*

## Appendix A — lazydap as built (capability inventory, code ground truth)

*(Agent report, 2026-07-31. Headline: after discounting commodity ingredients — JSON CLIs,
daemons, DAP clients, breakpoint features, skills, llms.txt — six claims survive: the
specified async→sync contract; DAP reachable from a bare Bash tool; visible adapter
normalisation; debuggee reaping with identity checks; breakpoints/watches as project state;
59 recorded decisions + 9 documented adapter quirks as a maintenance moat. Also flagged:
README/llms.txt claims were stale against shipped code — fixed in the same pass as this
dossier.)*

Key structural findings preserved for reference:

- **Command surface:** 24 top-level commands; `--wait`/`--timeout` on every
  execution command via one flattened struct so flags cannot drift; `--timeout` requires
  `--wait` (usage error otherwise); adapter inferred from file extension.
- **The `--wait` blob:** state/reason/raw_reason/thread_id/all_threads_stopped/
  additional_stopped_threads/hit_breakpoint_ids/exit_code/frame(prefetched)/
  captured_output(1 MB cap + `output_truncated`)/breakpoint_updates(latest-wins by id)/
  thread_updates/elapsed_ms. Timeout leaves the program running by design. 250 ms
  late-exit-code grace upgrades `terminated` → `exited`.
- **Mechanical distinctives:** sequence-numbered event ring shared by buffer and broadcast;
  subscribe-then-backlog with watermark resolution; delivery committed only when a blob is
  actually returned; pre-existing `Stopped` excluded from backlog so a `continue` is never
  named by the previous stop; 50 ms stopped-coalescing (D020); execution permit held for the
  whole run with `pause`/`disconnect` exempt (D021); wedged-adapter kill on unacknowledged
  execution requests; debuggee pid scraped from console text because codelldb never sends the
  DAP `process` event (quirk 9), identity-checked via `ps` before any kill (D045);
  `reason:"entry"` normalisation with `raw_reason` preserved, guard deliberately narrow
  (D033); stop-generation fencing on two-await inspection paths (D059).
- **Deliberate omissions:** no MCP server (named cession to the MCP field, re-evaluate at
  v0.2 — D023); one session (D007); no attach/restart; no CDP; no Windows; no telemetry;
  no crates.io (D051); `--wait` timeout never auto-pauses.

## Appendix B — competitive sweep (verbatim agent report)

*(Preserved as delivered; see the merge commit for full text. Structure: 19 player profiles
across five lanes — headless CLI, MCP servers, IDE-integrated, inversions/adjacent,
status-quo raw tools — plus a mechanism taxonomy table and cross-cutting findings.)*

The five findings that drive positioning:

1. **The `--wait` contract is genuinely rare.** Of ~30 tools examined, only
   microsoft/DebugMCP and go-delve/mcp-dap-server bundle stop-state into the responding
   call; none has block-until-stable-state with `--timeout`, `timeout`/`adapter_died`
   states, and captured stdout in one blob.
2. **Everything with traction requires a live IDE.** VS 2026, Junie, CLion's skill,
   DebugMCP. No major vendor ships headless; cloud sandbox agents can't hold sessions.
3. **MCP-vs-CLI wind favours shell-first:** Anthropic's own code-execution-with-MCP post
   (98.7% token reduction), Ronacher's essays, measured 4–32× token overhead;
   Microsoft now ships a skill alongside DebugMCP.
4. **Microsoft proved the premise with citable numbers** (debug-gym 37.2→48.4→52.1 on
   SWE-bench Lite; Debug2Fix: Sonnet 75.7→85.5 on GitBug-Java) — and Debug2Fix's design
   finding endorses mediated context-bundling over raw tool exposure.
5. **Naming hazard:** "AI debugger"/"agent debugging" SEO overwhelmingly means debugging
   *the agent*. "Let your agent drive a real debugger" is the uncontested framing. No
   product in the category has had a successful launch (best: 9 HN points) — the category
   is early, not won.

Nearest live threats in-lane: debug-skill (closest match; see verdict), debug-that
(traction + skill-slot occupancy; opposite bet on output format), with the tmux-sleep hack
as the true incumbent. ChatDBG (1.1k★) is the architectural inversion — LLM inside the
debugger for humans — and the space's proof of interest.

## Appendix C — refutation brief (verbatim agent report)

*(Preserved as delivered. Eight attack lines: gdb/MI, lldb, Delve, pdb/debugpy, MCP
debuggers, IDE agents, expect/pexpect, and same-shape prior art. Verdicts: lines 1–2 weaken
rhetorically but leave the specific claim standing — MI got the data model right in ~2000
and the transport wrong for shell-invoked agents; line 3 is the strongest legacy competitor
— Delve shipped blocking-continue at the RPC layer but has no output retrieval, no CLI, and
its own org built an MCP mediator; line 4 leaves the claim standing; line 5 partly
corroborates — the flagship MCP debugger's fire-and-forget continue is the anti-pattern
`--wait` exists to kill, and practitioner evidence favours CLI+skill; line 6: Cursor and
Visual Studio both* chose *non-conversational forms — market evidence the conversational
form is unsettled; line 7 weakens substantially — pexpect-driven lldb demonstrably works in
~7 tool calls, so the delta is convenience-plus-reliability, not raw capability; line 8
REFUTES the novelty framing via debug-skill/debugger-cli/dapi. Single most dangerous
argument: debug-skill, plus the category-scale concession that most agent coding succeeds
without dynamic debugging.)*
