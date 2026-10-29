#!/usr/bin/env python3
"""Render the measured SciFact sweep in docs/benchmarks for the README.

Requires matplotlib in the rendering environment, not in Vecnook.
Run from any directory: python3 tools/render_readme_benchmark.py
"""
from pathlib import Path
import json
import math

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.ticker as ticker

ROOT = Path(__file__).resolve().parents[1]
evidence = json.loads((ROOT / "docs/benchmarks/scifact-2026-10-29.json").read_text())
rows = evidence["summary"]
if len(rows) != 6 or len(evidence["runs"]) != 18 or not evidence.get("completed_at_utc"):
    raise ValueError("Expected a completed six-setting, three-round sweep")
ef = [r["ef"] for r in rows]
recall = [r["recall"] * 100 for r in rows]
latency = [r["p95_ms"] for r in rows]

bg, panel, grid, ink, muted = "#0d1117", "#111820", "#29343f", "#e6edf3", "#9baebf"
mint, blue = "#63d9b5", "#78b7ff"
plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 11,
                     "text.color": ink, "axes.labelcolor": muted,
                     "xtick.color": muted, "ytick.color": muted})
fig, axes = plt.subplots(1, 2, figsize=(12, 6.4), facecolor=bg)
fig.subplots_adjust(left=.075, right=.96, top=.69, bottom=.25, wspace=.28)
fig.text(.055, .92, "VECNOOK / MEASURED SEARCH", color=mint, fontsize=11, weight="bold")
fig.text(.055, .845, "The cost of higher recall", fontsize=25, weight="bold")
fig.text(.055, .782, "SciFact · 5,183 vectors · 768 dimensions · 300 queries · cosine · K=10",
         color=muted, fontsize=11)
x = list(range(len(ef)))
for ax in axes:
    ax.set_facecolor(panel)
    ax.spines[["top", "right", "left"]].set_visible(False)
    ax.spines["bottom"].set_color(grid)
    ax.grid(axis="y", color=grid, linewidth=.7)
    ax.set_axisbelow(True)
    ax.set_xticks(x, [str(v) for v in ef])
    ax.set_xlabel("efSearch (candidate pool)", labelpad=11)
    ax.tick_params(axis="both", length=0, pad=8)
    ax.set_xlim(-.55, 5.55)

axes[0].set_title("Search latency", loc="left", color=ink, fontsize=15, weight="bold", pad=20)
axes[0].bar(x, latency, color=blue, width=.55, zorder=3)
axes[0].errorbar(x, latency,
                 yerr=[[r["p95_ms"] - r["p95_min_ms"] for r in rows],
                       [r["p95_max_ms"] - r["p95_ms"] for r in rows]],
                 fmt="none", ecolor=ink, capsize=3, linewidth=1, zorder=4)
latency_top = max(r["p95_max_ms"] for r in rows) * 1.22
axes[0].set_ylim(0, latency_top)
axes[0].yaxis.set_major_locator(ticker.MaxNLocator(nbins=5))
axes[0].set_ylabel("p95 latency (ms) · lower is faster", labelpad=12)
for i, value in enumerate(latency):
    axes[0].text(i, rows[i]["p95_max_ms"] + latency_top * .025, f"{value:.3f}",
                 ha="center", fontsize=10, color=ink)

axes[1].set_title("Nearest-neighbor agreement", loc="left", color=ink, fontsize=15, weight="bold", pad=20)
axes[1].plot(x, recall, color=mint, linewidth=2.5, marker="o", markersize=6, zorder=3)
recall_floor = min(95, max(0, math.floor(min(recall)) - 1))
axes[1].set_ylim(recall_floor, 101.1)
axes[1].set_yticks(list(range(recall_floor, 101)))
axes[1].yaxis.set_major_formatter(ticker.PercentFormatter(xmax=100, decimals=0))
axes[1].set_ylabel(f"Recall@10 · axis starts at {recall_floor}%", labelpad=12)
axes[1].axhline(99, color=muted, linestyle=(0, (3, 4)), linewidth=.8, alpha=.65)
for i, value in enumerate(recall):
    axes[1].annotate(f"{value:.2f}%", (i, value), xytext=(0, 12 if i % 2 == 0 else -20),
                     textcoords="offset points", ha="center", fontsize=9, color=ink)

fig.text(.055, .135, f"Vecnook {evidence['version']} · 2026-10-29 · medians of 3 fresh processes; whiskers show min–max",
         fontsize=10, color=ink)
env = evidence["environment"]
rust_version = env["rustc"].split()[1]
fig.text(.055, .09, f"{env['cpu']} / {env['memory_bytes'] // 2**30} GiB / macOS {env['os']} / Rust {rust_version} release · sequential queries",
         fontsize=9, color=muted)
fig.text(.055, .05, "Excludes embeddings, file loading and index construction. Method and raw results: docs/current-benchmarks.md",
         fontsize=9, color=muted)
out = ROOT / "docs/media/search-benchmark.png"
fig.savefig(out, dpi=180, facecolor=bg)
plt.close(fig)
print(out)
