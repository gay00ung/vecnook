#!/usr/bin/env python3
"""Render the committed fixed-configuration engine comparison with Matplotlib."""
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker

ROOT = Path(__file__).resolve().parents[1]
evidence = json.loads((ROOT / "docs/benchmarks/engine-comparison-2026-10-29.json").read_text())
rows = evidence["summary"]
if len(rows) != 3 or len(evidence["runs"]) != 9 or not evidence.get("completed_at_utc"):
    raise ValueError("Expected three engines and nine completed runs")
bg, panel, grid, ink, muted = "#0d1117", "#111820", "#29343f", "#e6edf3", "#9baebf"
colors = ["#63d9b5", "#78b7ff", "#ffcd80"]
plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 11,
                     "text.color": ink, "axes.labelcolor": muted,
                     "xtick.color": muted, "ytick.color": ink})
fig, axes = plt.subplots(1, 2, figsize=(13, 6.6), facecolor=bg,
                         gridspec_kw={"width_ratios": [1.35, 1]})
fig.subplots_adjust(left=.25, right=.95, top=.64, bottom=.28, wspace=.27)
fig.text(.055, .925, "VECNOOK / LOCAL ENGINE COMPARISON", color=colors[0], fontsize=11, weight="bold")
fig.text(.055, .842, "Same data. Different local search APIs.", fontsize=24, weight="bold")
fig.text(.055, .777, "SciFact · 5,183 × 768 · 300 queries · cosine · K=10 · HNSW efSearch=128",
         fontsize=11, color=muted)
positions = [2, 1, 0]
labels = [f"{r['name']} {r['version']}\n{scope}" for r, scope in zip(
    rows, ["Rust index API", "Python index API", "Python local DB API"])]
for ax in axes:
    ax.set_facecolor(panel)
    ax.spines[["top", "right", "left"]].set_visible(False)
    ax.spines["bottom"].set_color(grid)
    ax.grid(axis="x", color=grid, linewidth=.7)
    ax.set_axisbelow(True)
    ax.set_ylim(-.5, 2.6)
    ax.set_yticks(positions)
    ax.tick_params(length=0, pad=12)
axes[0].set_yticklabels(labels, fontsize=11)
axes[1].set_yticklabels([])
axes[0].set_title("Query-call latency", loc="left", fontsize=15, weight="bold", pad=22)
axes[1].set_title("Nearest-neighbor agreement", loc="left", fontsize=14, weight="bold", pad=22)
for row, y, color in zip(rows, positions, colors):
    median = row["p95_ms"]
    axes[0].errorbar(median, y, xerr=[[median - row["p95_min_ms"]],
                                     [row["p95_max_ms"] - median]],
                     fmt="o", color=color, ecolor=color, capsize=5, markersize=8, linewidth=2)
    axes[0].annotate(f"{median:.3f} ms", (median, y), xytext=(0, 15),
                     textcoords="offset points", ha="center", color=ink, fontsize=11)
    recall = row["recall"] * 100
    axes[1].errorbar(recall, y, xerr=[[recall - row["recall_min"] * 100],
                                     [row["recall_max"] * 100 - recall]],
                     fmt="o", color=color, ecolor=color, capsize=5, markersize=8, linewidth=2)
    axes[1].annotate(f"{recall:.2f}%", (recall, y), xytext=(0, 15),
                     textcoords="offset points", ha="center", color=ink, fontsize=11)
axes[0].set_xscale("log")
axes[0].set_xlim(min(r["p95_min_ms"] for r in rows) * .6,
                 max(r["p95_max_ms"] for r in rows) * 1.65)
axes[0].xaxis.set_major_locator(ticker.LogLocator(base=10, subs=[1, 2, 5]))
axes[0].xaxis.set_major_formatter(ticker.FuncFormatter(lambda value, _: f"{value:g}"))
axes[0].xaxis.set_minor_locator(ticker.NullLocator())
axes[0].set_xlabel("p95 query-call latency (ms, log scale) · lower is faster", labelpad=12, fontsize=10)
axes[1].set_xlim(min(98, min(r["recall_min"] for r in rows) * 100 - .5), 100.25)
axes[1].set_xticks([98, 99, 100])
axes[1].set_xlabel("Recall@10 (%)", labelpad=12)
axes[1].axvline(99, color=muted, linestyle=(0, (3, 4)), linewidth=.8)
fig.text(.055, .16, "2026-10-29 · Apple M4 Pro / 48 GiB · 3 fresh processes per engine · medians with observed min–max",
         fontsize=10, color=ink)
fig.text(.055, .106, "Chroma includes local DB validation/query handling; Vecnook and USearch measure index calls. No network.",
         fontsize=9, color=muted)
fig.text(.055, .061, "M=16 · efConstruction=200 · one HNSW worker · f32 inputs · scope and raw results: docs/engine-comparison.md",
         fontsize=9, color=muted)
out = ROOT / "docs/media/engine-comparison.png"
fig.savefig(out, dpi=180, facecolor=bg)
plt.close(fig)
print(out)
