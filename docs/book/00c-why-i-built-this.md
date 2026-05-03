---
chapter: "0c"
title: Why I built this
status: complete
estimated_time_minutes: 8
---

# Chapter 00c — Why I built this

> Optional. ~8 minutes. Skip to [Chapter 01](01-cargo-workspaces.md) if you just want to learn Rust by building a thing. The technical context you need is in [Chapter 00b](00b-what-is-lazydap.md).

This is a longer answer than most book introductions need. If you read it, the design choices in lazydap and the way this book teaches will make a different kind of sense. If you skip it, you'll still learn Rust just fine.

## The gym for the brain

I have a theory that everyone needs a hobby that is *deliberately difficult*. Not a trade, not something you'll be paid for, not something with a deadline. Something that pushes your brain past what it can comfortably do, the way the gym pushes your body past what it can comfortably lift. Reading classical literature. Learning math. Learning a low-level programming language. Speaking a new natural language. Chess against humans. The category is wide.

Most of us want to *have done* the difficult thing. We want to be the person who reads in three languages, plays competent chess, has shipped a kernel module, can derive things from first principles. We just don't want to pay the tax. Modern life makes the avoidance easy. Work, home, chores, parenting, sleep — none of it stretches the mind in the particular way that learning a hard new thing stretches it. You can have a successful, productive career and still feel that your brain has plateaued.

The principle behind this is one I keep coming back to. Most people overestimate what they can achieve in a year and underestimate what they can achieve in ten. That gap between the two estimates is where compounding happens. The only way to get the ten-year compound is to put in the daily ten minutes, the daily hour, the daily uncomfortable reach, when there's no deadline forcing you to.

I've felt this before. When I first learned to code, the world was so new and so complicated that I genuinely felt I would never understand it. I'd struggle with a concept, push hard, read a thing, get it; come back the next day, find I'd lost most of it; push again, lose it again. For weeks, sometimes months, with the brain-hurts-because-it's-actually-stretching feeling. And then one day something clicked, the fragments fused into something solid, and suddenly I could read code I couldn't have read a month before. Once you cross that threshold, the gains compound exponentially. But you have to walk through the not-getting-it part to get there.

## What AI did to that practice

I lost it. Honestly.

For years I had the practice. I had a SaaS product I was building, I had a job, but I was *also* learning French — not because I was moving to France, just because I was curious. I learned ML for a while. I built grit, a Rust git rewrite, on a flight, with no internet, no AI, just the git internals book and reverse-engineering. (You can [look at it](https://github.com/bhekanik/grit) if you want; it's not a great codebase, but it taught me Rust string manipulation in a way no tutorial could have.) The point of all of those was the *learning*, not a deliverable.

Then AI showed up, and somewhere between "this is a useful tool" and "this is how I work now" I stopped doing the difficult thing for its own sake. Why would I? I could prompt for a solution faster than I could reason my way to one. Why would I read the book on structured logging when I could ask an LLM for a comprehensive guide and paste it into the next prompt? I caught myself doing exactly that recently — generated a guide to structured logging, didn't read it, fed it to a coding agent, the work got done. Comprehensive guide on my disk. I don't know what was in it.

That's not a usage of AI I want to keep. It works. It even produces decent output. But it skips the part where I learn, and over time the thing I'm bringing to the next prompt is less and less. That's a slow plateau and I can feel it.

There are two ways to use AI when you're learning.

The first is the way I just described: AI as a solution provider. Fast, productive, you produce more output, you understand less of it.

The second is AI as a *thinking companion*. You bring a half-formed thought; the AI plays it back and stress-tests it; you push back; you both refine; you walk away with a better thought *that you actually had*. The AI didn't give you the answer; it pulled it out of you. This is closer to a Socratic dialogue than to a Q&A. It's slower than the first mode, by a lot. But what you keep is yours.

This book is built around the second mode. The predict-before-run prompts, the surface-your-model prompts, the let-the-compiler-be-the-teacher discipline — those aren't pedagogy decoration. They're the operational difference between "the agent gives you Rust" and "you understand Rust, with the agent's help."

## Why C, why Rust, why this project

I wanted to feel the problems Rust was solving. I'd written enough Rust at the surface level to ship it (I maintain Rust code at work, I've done [grit](https://github.com/bhekanik/grit) and a couple of other projects), but I didn't feel I *understood* the language. I could use lifetimes; I couldn't articulate them. I could use macros; I couldn't tell you why they exist beyond "code generation." That's a particular kind of plateau, and it bothered me.

The route I picked was: go down a level. Learn C. Feel the problems first-hand. Then come back to Rust and have every feature land as a fix for a pain I'd actually felt. Use-after-free. The `errno` dance. NULL dereferences. Switch fall-through. Manual memory. Header file dance. Once those are *experiences* and not Wikipedia entries, Rust's ergonomics stop being trivia and start being a gift.

But I also wasn't doing this to *meet a deadline.* If learning C led me down to assembly, computer architecture, the kernel, and back to Rust ten years later — that would be fine. The point is the difficult-thing practice, not the destination. I'm not learning Rust to ship a product. I'm learning it to do a hard thing, daily, for as long as it takes.

What I struggled with: I learn best with a project. Just reading a book cover-to-cover doesn't stick. Web development is easy to project-ify (build an app, you'll hit the things you need to learn). C and OS-level work was less obvious to me. I poked at "build a shell in C" for a while — it's a great onramp because a shell is just a program that talks to the kernel, parses input, forks, waits, and renders output. Small surface, direct path to fork/exec/pipe/wait/dup2 (the same calls that show up as pain anchors throughout this book). Maybe I'll come back to the shell later. Or maybe I'll build a network-traffic inspector for macOS — Proxyman-but-mine — because the OS X search story is bad and I want to be able to inspect outgoing traffic from coding agents specifically. Either project would teach me what I want to learn. Both are still on the table.

What I landed on for *this* project was lazydap. Why?

Because I wanted to debug C from inside Neovim, and the existing options didn't fit me. nvim-dap is the standard, and it's fine, and a lot of people are happy with it, and I wasn't. Two reasons:

1. The keybinding model didn't compose with how I work. F5 to continue is fine when you're at a workstation. It's awkward when you're a Vim user living in modal commands. Configuration ergonomics were rough. Every project needed a launch config that's the JSON-ish offspring of VS Code's `.vscode/launch.json`, only different in subtle ways.
2. *Agents couldn't drive it.* This is the bigger one. I wanted to be able to say to a coding agent "set a breakpoint at line 42, run, and tell me what argc is when you hit it" and have it work. nvim-dap is a Lua plugin running inside Neovim. Agents can't drive Neovim. Even if they could, nvim-dap is built for keystrokes, not programmatic control.

What I wanted: a debugger I could drive from a shell. Every action a human can do, an agent or a script can also do. JSON output by default in non-TTY contexts. Stable schemas. A TUI on top of that for when I want eyes on the program, but the TUI is just another consumer of the same protocol, not the privileged surface. Built in Rust because the whole reason I was learning C was to come back to Rust with new eyes.

## Why "build it instead of fixing nvim-dap"

I asked myself this. Forking nvim-dap or contributing keybinding patches would have been more practical.

Honest answer: I'm building this primarily as a learning project. nvim-dap is a Lua plugin that wraps DAP. Forking it teaches me Lua and DAP. Building lazydap from scratch teaches me Rust and DAP and IPC and async event loops and TUI rendering and adapter abstraction and protocol design — every concept in modern Rust shows up in this project somewhere. The C journey gave me the motivation; the Rust journey gives me the tooling. Together, they make every "Rust solves *this* pain" moment a real conversation in my head, not a slogan from a tutorial.

This book is that conversation, written down, for whoever else might want to walk it.

## The principle behind the book's structure

If the *practice* is doing one difficult thing, daily, with no deadline — the book has to support that.

Each chapter is one teaching session. Each session is one new concept. Each concept lands with a runnable artifact at the end. The chapters are cumulative — chapter 04 builds on what chapter 03 left you with — so progress feels like motion, not theory piling up.

The predict-pauses and the teach-back questions exist because the *thinking-companion mode* requires them. If you read passively, the book degrades into a flat tutorial. If you stop at every 🔮 Predict and actually predict — even when you're alone with the book and no one's checking — you get the brain-hurts-because-it's-stretching feeling that the difficult-thing practice depends on. That feeling is the entire point. You can't have it on autopilot.

If you have an LLM agent driving the book live, the agent's job is to be the *thinking companion*, not the *solution provider* — the chapter is the curriculum, the agent's job is to add live responsiveness inside that script (calibrate to your actual prediction, run the actual code, read the actual compiler output). It does not freestyle, does not skip ahead, does not invent new concepts. It is, deliberately, slower than asking the same agent to "just write me a Rust process spawner." That's the trade.

## What you should take from this chapter

Two things, both optional:

**Build the thing you wished existed.** Better fuel than "ship a portfolio piece." Tends to produce better code, because you're the first user.

**Pick a difficult thing and do it daily.** Doesn't have to be Rust. Doesn't have to be programming at all. The point is the practice, the gym for the brain. A year of it doesn't get you much. Ten years gets you a different version of yourself. The trick is showing up when there's no deadline forcing you to.

That's why this book exists, and why it teaches the way it does.

Next up: [Chapter 01 — Cargo workspaces](01-cargo-workspaces.md). The book's setup is done; we start building.
