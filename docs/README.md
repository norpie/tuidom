# tuidom documentation

Four kinds of documentation live here, and each answers a different question. Knowing
which is which is most of the value — it is why none of them repeat each other, and the
rule that keeps them from drifting.

| Ask | Read |
|---|---|
| *What does this word mean here?* | [`GLOSSARY.md`](GLOSSARY.md) |
| *How do I do X?* | the guides below |
| *Does it exist yet?* | [`FEATURES.md`](FEATURES.md) |
| *What does this signature do?* | rustdoc — `cargo doc --open` |

And one more, for contributors: [`STYLE.md`](STYLE.md) — the conventions every change to
the codebase follows.

## The split rule

**The glossary defines. The guides explain. Neither does the other's job.**

A glossary entry is one or two sentences: enough to recognize the term and know which
guide owns it. It does not carry rationale, and it does not carry code.

A guide is task-ordered prose with code. When it needs a concept, it links to the glossary
rather than re-defining it. When it finds itself explaining what a term *means* for more
than a sentence, that belongs in the glossary and the guide should link instead.

This matters because the two drift silently otherwise: the same idea explained twice ages
at two different rates, and a reader has no way to tell which copy is current. One home
per fact.

`FEATURES.md` is checkboxes and nothing else. If it starts explaining *why* something
works the way it does, that reasoning belongs in a guide.

## Guides

Written as areas of the engine get documented. The set is being filled in; the
[glossary](GLOSSARY.md) is complete in the meantime and is the best conceptual reference
until a given guide exists.

- [**Getting started**](getting-started.md) — build a first application: document, tree,
  style, focus, input, and how to test it without a terminal
- [**Architecture**](architecture.md) — what a `Document` actually is, where state lives,
  how a frame happens, and the concurrency rules that follow from it

- [**Styling**](styling.md) — `Style`, inheritance, pseudo-states, borders, half-block
  edges, and the metadata escape hatches
- [**Colors**](colors.md) — OKLCH, variables, derivations, and the resolution order that
  makes `CurrentBg` non-circular
- [**Layout**](layout.md) — flexbox in terminal cells, positioning, reading computed
  geometry, and centering when the space is odd

- [**Events**](events.md) — dispatch and propagation, default actions and
  `prevent_default`, coalescing, and the events that report what the engine did
- [**Focus and selection**](focus-and-selection.md) — focus contexts and the focus stack,
  inert versus disabled, spatial navigation, and screen-wide text selection

<!-- Further guides are listed here as they land. -->

## Reading order

If you are new to tuidom, `README.md` in the repo root says what it is and why it exists.
From there the glossary is unusually readable front-to-back — it is organized by area
(core concepts, layout, styling, colors, events, focus, animation, rendering), not
alphabetically, so reading a section is a decent tour of that part of the engine.

For how the pieces fit together at runtime rather than what they are called,
`examples/demo.rs` exercises most of the surface in one file.

## A note on code in these docs

Snippets here are illustrative and are not compiled — the crate runs no doc tests, by
convention (see [`STYLE.md`](STYLE.md)). They are checked against real signatures when
written, but they can fall behind the API without anything failing. **Rustdoc and the test
suite are authoritative; these guides are not.** If a snippet and a signature disagree,
the signature is right and the snippet is a bug worth reporting.
