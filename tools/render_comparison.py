#!/usr/bin/env python3
"""Render all retained before/after comparison runs, with desktop/mobile layouts."""
import argparse
import json
from pathlib import Path
import statistics

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker

ROOT = Path(__file__).resolve().parents[1]
BG, PANEL, GRID, INK, MUTED = "#0d1117", "#111820", "#29343f", "#e6edf3", "#9baebf"
COLORS = ["#a5b2c1", "#63d9b5", "#78b7ff", "#ffcd80"]
LABELS = ["Vecnook before\nRC1 code · Rust index", "Vecnook after\nDevelopment · Rust index",
          "USearch 2.26.6\nPython index API", "Chroma 1.5.9\nPython local DB API"]


def validate(evidence):
    names = ["Vecnook before", "Vecnook after", "USearch", "Chroma"]
    rows = evidence["summary"]
    if not evidence.get("completed_at_utc") or [r["name"] for r in rows] != names:
        raise ValueError("Expected completed before/after and external-engine results")
    if len(evidence["runs"]) != 32:
        raise ValueError("Expected all 32 retained runs")
    for row in rows:
        runs = [r for r in evidence["runs"] if r["name"] == row["name"]]
        if len(runs) != 8 or any(r["incomplete_queries"] for r in runs):
            raise ValueError("Expected eight complete-query processes per target")
        for key in ["p95_ms", "recall"]:
            if abs(row[key] - statistics.median(r[key] for r in runs)) > 1e-12:
                raise ValueError("Summary differs from raw process metrics")
    return rows


def panel(ax, rows, metric, mobile):
    ax.set_facecolor(PANEL)
    ax.spines[["top", "right", "left"]].set_visible(False)
    ax.spines["bottom"].set_color(GRID)
    ax.grid(axis="x", color=GRID, linewidth=.8)
    ax.set_axisbelow(True)
    ax.set_ylim(-.55, 3.7)
    ax.set_yticks([3, 2, 1, 0])
    ax.tick_params(length=0, pad=10)
    ax.set_yticklabels((["Vecnook before\nRust · RC1 code", "Vecnook after\nRust · development", "USearch 2.26.6\nPython index API", "Chroma 1.5.9\nPython DB API"] if mobile else LABELS) if metric == "p95_ms" or mobile else [], fontsize=16 if mobile else 14)
    latency = metric == "p95_ms"
    ax.set_title("p95 query-call latency" if latency else "Recall@10", loc="left",
                 fontsize=16 if mobile else 19, weight="bold", pad=22)
    for row, y, color in zip(rows, [3, 2, 1, 0], COLORS):
        value = row[metric] if latency else row[metric] * 100
        low = row["p95_min_ms"] if latency else row["recall_min"] * 100
        high = row["p95_max_ms"] if latency else row["recall_max"] * 100
        ax.errorbar(value, y, xerr=[[value - low], [high - value]], fmt="o", color=color,
                    ecolor=color, capsize=5, markersize=8, linewidth=2)
        label = f"{value:.3f} ms" if latency else f"{value:.4f}%"
        ax.annotate(label, (value, y), xytext=(0, 13), textcoords="offset points",
                    ha="center", color=INK, fontsize=16 if mobile else 14)
    if latency:
        ax.set_xscale("log")
        ax.set_xticks([.5, 1, 2, 5, 10, 20])
        ax.xaxis.set_major_formatter(ticker.FuncFormatter(lambda v, _: f"{v:g}"))
        ax.xaxis.set_minor_locator(ticker.NullLocator())
        ax.set_xlim(.30, max(r["p95_max_ms"] for r in rows) * 1.4)
        ax.set_xlabel("ms · logarithmic scale · lower is faster", labelpad=13,
                      fontsize=10 if mobile else 13)
    else:
        ax.set_xlim(99.0, 100.25)
        ax.set_xticks([99, 99.5, 100])
        ax.set_xlabel("Exact-neighbor agreement (%)", labelpad=13,
                      fontsize=10 if mobile else 13)


def render(evidence, rows, output, mobile=False):
    plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 13,
                         "text.color": INK, "axes.labelcolor": MUTED,
                         "xtick.color": MUTED, "ytick.color": INK})
    reduction = 100 * (1 - rows[1]["p95_ms"] / rows[0]["p95_ms"])
    if mobile:
        fig, axes = plt.subplots(2, 1, figsize=(6.5, 13.2), facecolor=BG)
        fig.subplots_adjust(left=.37, right=.94, top=.70, bottom=.24, hspace=.58)
        fig.text(.055, .966, "VECNOOK / MEASURED SEARCH", color=COLORS[1], fontsize=12, weight="bold")
        fig.text(.055, .936, "Distance scoring,\nbefore and after", fontsize=24, weight="bold", va="top")
        fig.text(.055, .835, f"Vecnook median p95: {reduction:.1f}% lower", color=COLORS[1], fontsize=16)
        fig.text(.055, .807, "SciFact · 5,183 × 768 · 300 queries\nCosine · Top-10 · efSearch=128", color=MUTED, fontsize=12, va="top")
        footer = ["8 fresh processes per target · all 32 retained",
                  "Dots: median of process metrics; whiskers: min–max.",
                  "Host shared with ongoing QA; large outliers retained.",
                  "Chroma includes local DB handling; other paths are index APIs.",
                  "2026-10-29 · Apple M4 Pro · docs/engine-comparison.md"]
        for i, line in enumerate(footer):
            fig.text(.055, .145 - i * .023, line, fontsize=9, color=MUTED if i else INK)
    else:
        fig, axes = plt.subplots(1, 2, figsize=(14, 8.2), facecolor=BG,
                                 gridspec_kw={"width_ratios": [1.5, 1]})
        fig.subplots_adjust(left=.24, right=.96, top=.68, bottom=.27, wspace=.28)
        fig.text(.045, .94, "VECNOOK / MEASURED SEARCH", color=COLORS[1], fontsize=13, weight="bold")
        fig.text(.045, .863, "Distance scoring, before and after", fontsize=27, weight="bold")
        fig.text(.045, .805, f"Vecnook median p95: {rows[0]['p95_ms']:.3f} → {rows[1]['p95_ms']:.3f} ms  ·  {reduction:.1f}% lower",
                 fontsize=18, color=COLORS[1])
        fig.text(.045, .750, "SciFact · 5,183 × 768 · 300 queries · cosine · K=10 · M=16 · efConstruction=200 · efSearch=128",
                 fontsize=12, color=MUTED)
        fig.text(.045, .165, "8 fresh processes per target · all 32 retained · medians with observed min–max",
                 fontsize=12, color=INK)
        fig.text(.045, .111, "Host shared with ongoing QA; large outliers retained. Chroma includes local DB handling; API scopes differ.",
                 fontsize=11, color=MUTED)
        fig.text(.045, .060, "2026-10-29 · Apple M4 Pro / 48 GiB · after = unpublished development build · docs/engine-comparison.md",
                 fontsize=10, color=MUTED)
    for ax, metric in zip(axes, ["p95_ms", "recall"]):
        panel(ax, rows, metric, mobile)
    fig.savefig(output, dpi=160, facecolor=BG)
    plt.close(fig)
    print(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, default=ROOT / "docs/benchmarks/engine-comparison-before-after-2026-10-29.json")
    args = parser.parse_args()
    evidence = json.loads(args.input.read_text())
    rows = validate(evidence)
    render(evidence, rows, ROOT / "docs/media/engine-comparison.png")
    render(evidence, rows, ROOT / "docs/media/engine-comparison-mobile.png", mobile=True)


if __name__ == "__main__":
    main()
