#!/usr/bin/env python3
"""Build pitch_deck.pptx from deck_template.pptx.

Fills the 5 official slides in place and duplicates the
technical-workflow layout for 3 extra content slides (Composed
architecture, Progress & proof, Market & model), renumbering the
PLATFORM/PITCH-STEP/NEXT chrome so the metro motif stays consistent.

Run: ../.venv/bin/python build_deck.py   (from this directory)
Output: pitch_deck.pptx (upload to the portal; ≤20 MiB)
"""
import copy
import sys
from pathlib import Path

from pptx import Presentation
from pptx.oxml.ns import qn
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.util import Emu, Pt

ROOT = Path(__file__).resolve().parent
TPL, OUT = ROOT / "deck_template.pptx", ROOT / "pitch_deck.pptx"

IN = lambda v: Emu(int(v * 914400))


def font_of(shape):
    for p in shape.text_frame.paragraphs:
        for r in p.runs:
            col = None
            try:
                col = r.font.color.rgb
            except Exception:
                pass
            return dict(name=r.font.name, size=r.font.size,
                        bold=r.font.bold, color=col)
    return dict(name=None, size=None, bold=None, color=None)


def set_text(shape, lines, scale=1.0, size=None):
    f = font_of(shape)
    if size is not None:
        f["size"] = size
    elif scale != 1.0 and f["size"]:
        f["size"] = Emu(int(f["size"] * scale))
    tf = shape.text_frame
    tf.clear()
    if isinstance(lines, str):
        lines = [lines]
    for i, line in enumerate(lines):
        p = tf.paragraphs[0] if i == 0 else tf.add_paragraph()
        r = p.add_run()
        r.text = line
        if f["name"]:
            r.font.name = f["name"]
        if f["size"]:
            r.font.size = f["size"]
        if f["bold"] is not None:
            r.font.bold = f["bold"]
        if f["color"]:
            r.font.color.rgb = f["color"]


def duplicate_slide(prs, idx):
    src = prs.slides[idx]
    dest = prs.slides.add_slide(src.slide_layout)
    for shp in list(dest.shapes):
        shp._element.getparent().remove(shp._element)
    for shp in src.shapes:
        dest.shapes._spTree.append(copy.deepcopy(shp._element))
    rid_map = {}
    for rel in src.part.rels.values():
        if rel.reltype.endswith("/image"):
            rid_map[rel.rId] = dest.part.relate_to(
                rel.target_part, rel.reltype)
    for blip in dest.shapes._spTree.iter(qn("a:blip")):
        rid = blip.get(qn("r:embed"))
        if rid in rid_map:
            blip.set(qn("r:embed"), rid_map[rid])
    return dest


def reorder(prs, order):
    lst = prs.slides._sldIdLst
    els = list(lst)
    for e in els:
        lst.remove(e)
    for i in order:
        lst.append(els[i])


def roles(slide):
    """Map template shapes to roles by their (constant) positions."""
    out = {}
    for sh in slide.shapes:
        if not sh.has_text_frame or not sh.text_frame.text.strip():
            continue
        top = Emu(sh.top).inches
        txt = sh.text_frame.text
        if top < 0.8:
            if "PLATFORM" in txt:
                out["platform"] = sh
            elif "NEXT" in txt or "FINAL" in txt:
                out["next"] = sh
        elif top < 2.0 and "PITCH STEP" in txt:
            out["step"] = sh
        elif top < 3.8:
            out["title"] = sh
        elif top < 5.0:
            out["lead"] = sh
        elif top < 7.5:
            out["body"] = sh
        elif top < 9.0:
            out["strip"] = sh
        elif top < 10.5 and "©" not in txt:
            out["meta"] = sh
    return out


# ---------------- content ----------------
# body: ≤ ~6 short lines (font auto-shrinks >4 lines)

CONTENT = {
    "cover": dict(
        pill="STELLAR COMPOSED · MADE TO MOVE",
        title=["Y.A.R.V."],
        mid="concentrated liquidity for the lazy (and everyone else)",
        sub="yet-to-be-named automated rebalancing vaults — live on Stellar testnet",
    ),
    "problem": dict(
        title="THE PROBLEM",
        lead="CL turned Stellar LPs into order managers.",
        body=[
            "CL positions earn only while price sits inside the tick band —",
            "drift out and the position earns nothing AND holds 100%",
            "of the losing token.",
            "The only fix on Stellar today: a manual withdraw → swap →",
            "deposit routine where one bad execution costs more than",
            "days of fees.",
        ],
        strip="👉 We lived it: on a real testnet rebalance, one route quoted ~38% off fair price — our on-chain price bound reverted it. A manual LP would have eaten the loss.",
    ),
    "solution": dict(
        title="THE SOLUTION",
        lead="Deposit either pool token, hold shares, withdraw to either or both — one tx each way.",
        body=[
            "The strategy contract owns the Aquarius CL position; a keeper",
            "automatically rebalances it when price drifts out of range —",
            "inside strict on-chain policy. You never touch a band again.",
            "Zap-in/out: a deposit accepts either token and a withdraw can",
            "pay out a chosen split — swap+deposit batch atomically, one tx.",
            "LP fees and AQUA rewards compound back into the share price.",
            "It's a platform: third-party strategists run their own",
            "rebalancing templates and earn the operator fee leg.",
        ],
        strip="👉 One transaction in · one transaction out · no ranges, no bundles, no trigger config.",
    ),
    "composed": dict(
        title="STELLAR COMPOSED",
        lead="Three protocols composed — plus a layer given back.",
        body=[
            "DeFindex — OtterSec-audited vault + factory: custody, shares,",
            "roles (our own factory instance; vault_fee pinned 0).",
            "Aquarius — concentrated pools hold the managed position.",
            "Reflector — oracle referees every price: balance(), swap",
            "minimums, staleness gates. Never the pool's own spot.",
            "Back to the ecosystem: first CL strategy for DeFindexTrait +",
            "P1/P2 vault bugs found, patched and disclosed upstream.",
        ],
        strip="👉 DeFindex custody × Aquarius liquidity × Reflector referee — the textbook composed build.",
    ),
    "workflow": dict(
        title="TECHNICAL WORKFLOW",
        lead="How a keeper rebalance runs — and why it can't hurt you.",
        body=[
            "A keeper proposes each rebalance; the contract re-checks",
            "it against stored limits and Reflector prices — keeper",
            "numbers are never trusted.",
            "Unwind → swap → re-mint is one atomic transaction —",
            "a rebalance can't land half-done.",
            "Swap routing is purpose-built for the rebalance step —",
            "the residual leg is routed for maximum swap efficiency.",
        ],
        strip="👉 The contract itself re-checks every keeper action against 14 stored limits — rival vaults check far fewer (Charm: 5, Gamma: none — it trusts the owner).",
    ),
    "proof": dict(
        title="PROGRESS & PROOF",
        lead="The whole share-token + platform layer shipped this weekend.",
        body_scale=0.72,
        body=[
            "New this weekend: six contracts live on Stellar testnet —",
            "strategy, registry, platform, routing, test scaffolding.",
            "Verified on-chain: create → deposit → harvest → withdraw;",
            "harvest really deployed a CL position, converted rewards",
            "and skimmed the fee — hashes in DEPLOYMENTS.md.",
            "Tested hard: 28 scenario tests + real-vault end-to-end,",
            "37 audit warnings → 0, fuzzing clean — plus a replay of",
            "the real mainnet pool and oracle from a chain snapshot.",
        ],
        strip="👉 Disclosed prior work: offchain CL rebalance algo → PoC vault (Oct 2) — everything else is new.",
    ),
    "market": dict(
        title="MARKET & ROADMAP",
        lead="The vault shape that won everywhere — ported to Stellar.",
        body_scale=0.72,
        body=[
            "Won on Solana + EVM; Stellar's open slot.",
            "Model: capped cut of position income, split",
            "platform/operator; the vault fee stays at 0.",
            "",
            "Next: extensive testing → Sushi adapter →",
            "mainnet live-fire → Axis orderbook MM",
            "vaults → keeper bounties → SCF.",
        ],
        strip="👉 First-mover CL vault platform on Stellar — the strategy layer DeFindex itself hasn't built.",
    ),
    "team": dict(
        title="THE BUILDER",
        lead="One builder — and a disclosed AI toolchain.",
        body=[
            "Trond — years of market making with synt.tech on the",
            "Stellar DEX; already operating automated CL rebalancing",
            "with an offchain algo. Architecture & security decisions.",
            "AI tooling disclosed: Devin SWE-2 Max agents for research,",
            "code, tests, docs and deck/video assets — OMP DeepSeek V4.1",
            "Flash 1017 + Kimi 3.1 for code auditing.",
        ],
        strip="👉 Repo · testnet deployment · demo video — links in the submission. Judges: ask me anything on Discord.",
    ),
}

# order: source indices after appending 3 dups of slide 4 (idx 3)
# template: 0 cover, 1 solution, 2 pmf, 3 workflow, 4 team,
#           5 dup(composed), 6 dup(proof), 7 dup(market)
ORDER = [0, 2, 1, 5, 3, 6, 7, 4]
KEYS = ["cover", "problem", "solution", "composed",
        "workflow", "proof", "market", "team"]
NEXT_LABEL = {"problem": "NEXT ▸ THE SOLUTION",
              "solution": "NEXT ▸ STELLAR COMPOSED",
              "composed": "NEXT ▸ TECHNICAL WORKFLOW",
              "workflow": "NEXT ▸ PROGRESS & PROOF",
              "proof": "NEXT ▸ MARKET & MODEL",
              "market": "NEXT ▸ THE BUILDER",
              "team": "FINAL STOP"}
STEP_TOTAL = len(KEYS) - 1


def main():
    prs = Presentation(TPL)
    for _ in range(3):
        duplicate_slide(prs, 3)
    reorder(prs, ORDER)

    for i, (key, slide) in enumerate(zip(KEYS, prs.slides)):
        c = CONTENT[key]
        r = roles(slide)
        if key == "cover":
            for sh in slide.shapes:
                if not sh.has_text_frame:
                    continue
                t = sh.text_frame.text
                if "PITCH DECK" in t:
                    set_text(sh, c["pill"])
                elif "HACKATHON" in t:
                    set_text(sh, c["title"])
                elif "#HackMeridian26" in t:
                    set_text(sh, "#HackMeridian26", scale=0.8)
                elif "PLATFORM" in t:
                    set_text(sh, f"PLATFORM 01 / {len(KEYS):02d}")
                elif "October 2026" in t:
                    set_text(sh, "October 2026 · Lisbon")
            if c.get("mid"):
                sub = slide.shapes.add_textbox(IN(1.05), IN(5.4),
                                               IN(12.0), IN(1.4))
                r0 = sub.text_frame.paragraphs[0].add_run()
                r0.text = c["mid"]
                r0.font.size = Pt(26)
                r0.font.color.rgb = RGBColor(0xFF, 0xD2, 0x3F)
                if c.get("sub"):
                    p2 = sub.text_frame.add_paragraph()
                    p2.space_before = Pt(10)
                    r1 = p2.add_run()
                    r1.text = c["sub"]
                    r1.font.size = Pt(16)
                    r1.font.color.rgb = RGBColor(0xAA, 0xAA, 0xAA)
            continue
        if "platform" in r:
            set_text(r["platform"], f"PLATFORM {i + 1:02d} / {len(KEYS):02d}")
        if "next" in r:
            set_text(r["next"], NEXT_LABEL[key])
        if "step" in r:
            set_text(r["step"], f"PITCH STEP {i:02d} / {STEP_TOTAL:02d}")
        if "title" in r:
            set_text(r["title"], c["title"])
        if "lead" in r:
            set_text(r["lead"], c["lead"], scale=0.85)
        if "body" in r:
            set_text(r["body"], c["body"],
                     scale=c.get("body_scale",
                                 0.75 if len(c["body"]) > 6 else 0.85))
        if "strip" in r:
            set_text(r["strip"], c["strip"], scale=0.8)
        if key == "problem":
            r["body"].width = IN(9.0)
            shot = ROOT / "assets/screenshots/aquarius_oor_card.png"
            if shot.exists():
                w = 8.6
                slide.shapes.add_picture(str(shot), IN(10.3), IN(3.95),
                                         width=IN(w), height=IN(w / 2.826))
                cap = slide.shapes.add_textbox(IN(10.3), IN(7.05),
                                               IN(8.6), IN(0.6))
                f = font_of(r["body"])
                cap.text_frame.word_wrap = True
                for i, line in enumerate([
                        "Real Aquarius testnet position — 20 XLM, range",
                        "4.48–4.95, price at 5.57 → fees $0. Manually",
                        "babysitting CL positions is a pain in the ass."]):
                    p = cap.text_frame.paragraphs[0] if i == 0 \
                        else cap.text_frame.add_paragraph()
                    run = p.add_run()
                    run.text = line
                    if f["name"]:
                        run.font.name = f["name"]
                    run.font.size = Pt(13)
                    run.font.italic = True
                    if f["color"]:
                        run.font.color.rgb = f["color"]

        if key == "composed":
            shot = ROOT / "assets/composed_figure.png"
            if shot.exists():
                w = 7.6
                slide.shapes.add_picture(str(shot), IN(11.6), IN(3.9),
                                         width=IN(w),
                                         height=IN(w * 1100 / 1700))

        if key == "proof":
            r["lead"].width = IN(9.2)
            r["body"].width = IN(9.2)
            r["strip"].width = IN(10.0)
            src = ROOT / "video/shots/rebal_06_tick600_drifting.png"
            if src.exists():
                from PIL import Image
                crop = ROOT / "assets/screenshots/app_live_drift.png"
                Image.open(src).crop((470, 0, 1655, 600)).save(crop)
                w = 8.2
                h = w * 600 / 1185
                pic = slide.shapes.add_picture(
                    str(crop), IN(11.0), IN(3.85),
                    width=IN(w), height=IN(h))
                pic.click_action.hyperlink.address = \
                    "https://synt.tech/rebalancing_vault"
                url = slide.shapes.add_textbox(
                    IN(11.0), IN(3.85 + h + 0.2), IN(8.2), IN(0.55))
                f = font_of(r["body"])
                run = url.text_frame.paragraphs[0].add_run()
                run.text = "synt.tech/rebalancing_vault"
                run.hyperlink.address = \
                    "https://synt.tech/rebalancing_vault"
                if f["name"]:
                    run.font.name = f["name"]
                run.font.size = Pt(22)
                run.font.bold = True
                run.font.underline = True
                if f["color"]:
                    run.font.color.rgb = f["color"]

        if key == "market":
            f = font_of(r["body"])
            ink, gray = RGBColor(0x1A, 0x1A, 0x1A), RGBColor(0x66, 0x66, 0x66)

            def style(run, size, bold=False, color=ink):
                if f["name"]:
                    run.font.name = f["name"]
                run.font.size = Pt(size)
                run.font.bold = bold
                run.font.color.rgb = color

            tb = slide.shapes.add_textbox(IN(13.85), IN(4.05),
                                          IN(5.55), IN(2.4))
            tf = tb.text_frame
            tf.word_wrap = True
            run = tf.paragraphs[0].add_run()
            run.text = "THE TEMPLATE LIBRARY — 5 FAMILIES"
            style(run, 13, bold=True)
            for name, gloss in [
                    ("re-center-on-exit", "re-mint the band on drift"),
                    ("inventory-ratio", "hold an asset mix (live!)"),
                    ("peg-anchored", "stable pairs, median peg"),
                    ("signal-driven", "off-chain vol-scaled plans"),
                    ("no-swap reuse", "park leftover, never sell")]:
                p = tf.add_paragraph()
                p.space_before = Pt(8)
                style(p.add_run(), 15, bold=True)
                p.runs[0].text = name
                r2 = p.add_run()
                r2.text = f" — {gloss}"
                style(r2, 11.5, color=gray)

            vh = slide.shapes.add_textbox(IN(13.85), IN(6.30),
                                          IN(5.0), IN(0.4))
            run = vh.text_frame.paragraphs[0].add_run()
            run.text = "VENUES ON THE ROADMAP"
            style(run, 11, bold=True, color=gray)

            for i, (img, nm, st) in enumerate([
                    ("aquarius.png", "AQUARIUS", "live on testnet"),
                    ("sushi.png", "SUSHI V3", "adapter next"),
                    ("axis.png", "AXIS", "orderbook MM vaults")]):
                x = 13.85 + i * 1.85
                logo = ROOT / "assets/logos" / img
                if not logo.exists():
                    continue
                chip = slide.shapes.add_shape(
                    MSO_SHAPE.ROUNDED_RECTANGLE,
                    IN(x), IN(6.70), IN(1.70), IN(1.40))
                chip.fill.solid()
                chip.fill.fore_color.rgb = RGBColor(0xFF, 0xFF, 0xFF)
                chip.line.color.rgb = ink
                chip.line.width = Pt(0.75)
                ctf = chip.text_frame
                ctf.word_wrap = True
                ctf.vertical_anchor = MSO_ANCHOR.BOTTOM
                ctf.margin_left = ctf.margin_right = IN(0.05)
                rn = ctf.paragraphs[0].add_run()
                rn.text = nm
                style(rn, 10.5, bold=True)
                ctf.paragraphs[0].alignment = PP_ALIGN.CENTER
                p2 = ctf.add_paragraph()
                p2.alignment = PP_ALIGN.CENTER
                rs = p2.add_run()
                rs.text = st
                style(rs, 8.5, color=gray)
                slide.shapes.add_picture(
                    str(logo), IN(x + (1.70 - 0.55) / 2), IN(6.82),
                    width=IN(0.55), height=IN(0.55))

    prs.save(OUT)
    print(f"{OUT}  {OUT.stat().st_size / 1e6:.2f} MB, {len(prs.slides.__iter__.__self__._sldIdLst)} slides")
    # simpler slide count:
    print("slides:", len(prs.slides._sldIdLst))


if __name__ == "__main__":
    sys.exit(main())
