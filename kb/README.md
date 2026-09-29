# Knowledge base

Shared context for humans and coding agents working on `kani-stylus`.

Read these in order when picking up the project cold:

| File | What it holds |
| --- | --- |
| [00-project.md](00-project.md) | What we're building, repo layout, where new code goes |
| [10-links.md](10-links.md) | Canonical upstream docs — every URL here was checked for a 200 |
| [20-stylus.md](20-stylus.md) | How Stylus contracts and the ArbOS host interface actually work |
| [30-kani.md](30-kani.md) | What Kani can and can't verify, and how it's driven |
| [35-arithmetic-oracle.md](35-arithmetic-oracle.md) | Why `U256` `*` and `/` defeat SAT; exact division vs. the lemma-based oracle, and when to use which |
| [40-toolchain.md](40-toolchain.md) | Local versions, environment gotchas |
| [50-feasibility.md](50-feasibility.md) | Proposal claims vs. what the source says; open questions |
| [60-roadmap.md](60-roadmap.md) | What's next and in what order; why it differs from the proposal |

## Ground rules for agents

- **Facts in this KB are dated and sourced.** If a note says "verified against
  stylus-sdk 0.10.9", check the version still matches before relying on it.
- **Prefer reading vendored crate source over recalling API shapes.** The
  dependency sources sit under
  `~/.cargo/registry/src/index.crates.io-*/stylus-{core,sdk,proc}-0.10.9/`.
  They are the ground truth for the `Host` trait and the storage model.
- **Don't copy upstream docs into this repo.** Link to them ([10-links.md](10-links.md))
  and record only the delta: what we learned, decided, or found wrong.
- **[50-feasibility.md](50-feasibility.md) is the live risk register.** When a
  question there gets answered by an experiment, replace the question with the
  answer and note how it was verified.
