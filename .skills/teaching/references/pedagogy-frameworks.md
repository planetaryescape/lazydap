# Pedagogy frameworks the operating rules come from

Citations and applications. Read this when you want the *why* behind a rule.

## PRIMM — Predict, Run, Investigate, Modify, Make

**Sue Sentance, 2017.** Programming pedagogy framework with empirical validation against control groups in CS education research.

The cycle:

1. **Predict** — learner predicts what given code will do, before running it
2. **Run** — execute the code; compare prediction with reality
3. **Investigate** — when prediction was wrong, dig into why
4. **Modify** — make a change to the code; predict, run, investigate again
5. **Make** — write a new program from scratch using what was learned

The Predict step is the **diagnostic**. Wrong predictions surface mental-model errors that "explain then run" hides.

**Why it matters for senior engineers:** they can fake confidence. Prediction extracts the actual model.

**How we apply it:**
- Every code snippet shown gets a "what does this do?" prompt before execution
- Wrong predictions are gold — slow down and dig in
- "Modify" maps to gradual release round 2 ("we do")
- "Make" maps to gradual release round 3 ("you do")

**Sources:**
- Sentance — [PRIMM project](https://suesentance.net/primm-project/)
- Sentance — [PRIMM intro post](https://suesentance.net/2017/09/01/primm-a-structured-approach-to-teaching-programming/)

## Gradual Release of Responsibility (GRR) — "I do, we do, you do"

**Pearson & Gallagher, 1983.** Originally an education framework; widely applied to programming pedagogy.

The release ladder:

1. **I do** — instructor demonstrates with full narration; learner observes
2. **We do** — instructor and learner work together; instructor scaffolds
3. **You do** — learner works independently; instructor reviews
4. **You teach** (optional terminal step) — learner explains to someone else

**Application to pair programming:**

| Round | Who types | Instructor's role | Learner's role |
|---|---|---|---|
| I do | Instructor | Write + narrate decisions out loud | Observe, ask clarifying questions |
| We do | Both | Write skeleton + leave the conceptually-meaty part blank | Fill the meaty part |
| You do | Learner | Watch quietly; respond when asked | Write from scratch |
| You teach | Learner | Pretend to be a fresh colleague | Explain the concept in their own words |

**Skipping "we do" is the most common failure mode.** "You do" tasks dropped immediately after demonstration are too hard; learner stalls. The bridge round matters.

**Sources:**
- [GCU: How to use I do, we do, you do for programming](https://www.gcu.edu/blog/engineering-technology/how-use-i-do-we-do-you-do-teaching)
- [NSW Education on GRR](https://education.nsw.gov.au/teaching-and-learning/curriculum/explicit-teaching/explicit-teaching-strategies/gradual-release-of-responsibility)

## Use-Modify-Create (UMC)

**Lee et al., 2011** (ACM Inroads). CS-education-specific variant of GRR.

Three stages:

1. **Use** — learner runs working code, observes behaviour
2. **Modify** — learner changes parts of the code; observes effects
3. **Create** — learner writes their own from scratch

This is the same pattern as PRIMM's last three steps and GRR's three rounds. Different field, same insight: the modification step in the middle is what makes the leap to creation possible.

For pair programming, treat UMC as another formulation of "I do → we do → you do" and apply identically.

**Sources:**
- Lee et al. — [Use-Modify-Create paper, ACM Inroads](https://dl.acm.org/doi/10.1145/1929887.1929902)

## Cognitive Load Theory (CLT)

**John Sweller, 1988+.** Learning is constrained by working memory capacity. Three sources of load:

- **Intrinsic load** — inherent complexity of the topic itself
- **Extraneous load** — friction from how it's presented (bad analogies, missing context, fancy build systems, distractions)
- **Germane load** — schema-building (the productive load that creates long-term understanding)

The constraint: **total load can't exceed working memory.** When it does, learning collapses — the learner feels "I understood each piece but can't put it together."

**Implications:**

- Rust's intrinsic load is high (ownership + types + lifetimes interact)
- Cut extraneous load: minimal build system, one new crate at a time, no fancy IDE features
- One new concept per session keeps intrinsic load bounded
- Gradual release shifts load to germane (schema-building) and away from intrinsic struggle

**Sources:**
- Sweller — [element interactivity, Springer](https://link.springer.com/article/10.1007/s10648-010-9128-5)
- [NSW CESE primer (PDF)](https://education.nsw.gov.au/content/dam/main-education/about-us/educational-data/cese/2017-cognitive-load-theory.pdf)

## Worked-example fading (Renkl & Atkinson)

**Renkl & Atkinson, 2003.** Show full worked example, then progressively blank out steps for the learner to fill in, then full independent problem.

Critical: fade the *concept you just taught*, not arbitrary lines. If you taught ownership transfer, fade the transfer logic in subsequent examples — not the boilerplate.

Maps directly to gradual release. Worked example = "I do." Faded example = "we do." Independent problem = "you do."

**Sources:**
- Renkl & Atkinson — [worked-example fading, Springer](https://link.springer.com/article/10.1023/B:TRUC.0000021815.74806.f6)
- Wikipedia — [worked-example effect](https://en.wikipedia.org/wiki/Worked-example_effect)

## Andragogy (Adult Learning)

**Malcolm Knowles, 1968+.** Six assumptions about how adults learn that distinguish them from children:

1. **Need to know** — adults need to know *why* before *how*
2. **Self-concept** — adults are self-directed; respect that
3. **Experience** — adults bring rich prior experience; mine it, don't bypass it
4. **Readiness** — adults learn what they need *now*, not what's "next in the curriculum"
5. **Orientation** — problem-centred, not subject-centred
6. **Motivation** — primarily intrinsic

For senior engineers specifically:
- Always lead with *why* this exists ("we need lifetimes because the compiler must prove no dangling references")
- Don't patronise (skip "what is a function")
- Frame concepts as solutions to problems they recognise
- Anchor to their existing experience explicitly

**Sources:**
- [Malcolm Knowles overview, infed.org](https://infed.org/dir/welcome/malcolm-knowles-informal-adult-education-self-direction-and-andragogy/)
- [Maestro summary](https://maestrolearning.com/blogs/malcolm-knowles-five-assumptions-of-learners-and-why-they-matter/)

## Zone of Proximal Development (ZPD)

**Lev Vygotsky, 1930s.** ZPD = what learner can do with help but not alone. Teaching that targets this zone is most effective; teaching below it is wasted, teaching above it is overwhelming.

For senior engineers learning a new language:
- ZPD is **wider** in areas with strong analogs (HTTP, JSON, async at the API level)
- ZPD is **narrower** in areas with no analog (lifetimes, stack-vs-heap, marker traits)
- ZPD has **cliffs** — a senior with deep adjacent knowledge can be at PhD level on one topic and primary-school level on another

Implication: probe before assuming. Don't assume because they got X they'll get Y. The cliff is real.

## Expert Blind Spot

**Nathan, Koedinger & Alibali, 2001.** Experts compress their reasoning steps and forget what's hard. They misjudge what novices need.

**Doubly bad for teaching senior engineers:** the teacher (you) has expert blind spots, AND the learner has expert blind spots in their *prior* language that they'll project onto the new one without flagging.

**Counter-move:** Rule 1 (surface their model first) before teaching. The articulation surfaces both blind spots — yours and theirs — so the conversation can correct them.

**Sources:**
- [Expert Blind Spot, Nathan et al. PDF](http://pact.cs.cmu.edu/koedinger/pubs/2001_NathanEtAl_ICCS_EBS.pdf)
- [IU CITL on expert blind spots](https://blogs.iu.edu/citl/2023/04/10/reflecting-on-expert-blind-spots-to-improve-skills-based-teaching/)

## Productive Struggle

**ASCD framing.** The "sweet spot" of difficulty: hard enough to require effort and engagement, not so hard it breaks confidence. Productive struggle is where learning consolidates.

**Why it matters here:** if I (the AI) type all the code, the learner has zero productive struggle. They watch, nod, and learn nothing about ownership. The "you do" round must put their hands on the keyboard for the conceptual decisions.

**The frustration is the curriculum.** A learner who never hits a borrow-checker error never internalises the borrow checker. Engineering productive struggle (rather than handing them the answer) is the whole job.

**Sources:**
- [ASCD on productive struggle](https://www.ascd.org/el/articles/productive-struggle-is-a-learners-sweet-spot)

## Predict-then-test (the diagnostic move)

Empirically validated: prediction-before-execution improves learning outcomes vs. write-from-scratch.

The mechanism: prediction surfaces the mental model. A wrong prediction is more diagnostic than a successful execution because it reveals what the learner *thought*, not just what they *can produce by trial and error*.

For senior learners specifically, this is essential — they have the skill to fix code by trial and error without ever surfacing the model that's broken. Prediction extracts the model.

**Sources:**
- [Prediction vs production, ScienceDirect](https://sciencedirect.com/science/article/abs/pii/S0959475223001408)
- [Predict Before You Run, Runestone](https://runestone.academy/ns/books/published/fopp/GeneralIntro/WPPredictBeforeYouRun.html)

## See One, Do One, Teach One

**William Halsted, Johns Hopkins, 1890s.** Surgical training framework. Modern critique: one observation isn't enough for safety in surgery — but the underlying *gradient* (observe → assisted → independent → teach) is sound.

For programming pair-teaching:
- "See one" → I do
- "Do one" → we do + you do
- "Teach one" → end-of-session teach-back

The terminal "teach" step is the asymptote. If the learner can teach the concept, they own it.

**Sources:**
- [PMC: see one do one teach one in surgical training](https://pmc.ncbi.nlm.nih.gov/articles/PMC4785880/)

## Synthesis — what to remember

If you forget everything else:

1. **Predict before run** (PRIMM) — diagnostic
2. **I do → we do → you do** (GRR) — laddered responsibility
3. **One concept per session** (CLT) — load discipline
4. **Surface their model first** (Andragogy + Expert Blind Spot) — anchor before extending
5. **Productive struggle on conceptual stakes** — they type for the meaningful decisions
6. **Compiler as co-teacher** (Klabnik) — don't pre-empt
7. **Teach back at end** (See/Do/Teach) — articulation locks retention
