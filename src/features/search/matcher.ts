import { getCategoryLabel, TOOLS } from "../tool-registry/registry";
import type { ToolDefinition } from "../tool-registry/types";

/**
 * Hand-rolled fuzzy matcher — small, deterministic, dependency-free.
 * Scored subsequence match: contiguous runs and word-boundary hits score
 * higher than sparse matches across the same field; name counts double.
 */

/** Score one field. Higher is better; null means "no match". */
export function fuzzyScore(query: string, field: string): number | null {
  const q = query.trim().toLowerCase();
  if (!q) return null;
  const target = field.toLowerCase();

  let score = 0;
  let targetIndex = 0;
  let run = 0;
  let previousHitIndex = -1;

  for (const queryChar of Array.from(q)) {
    let hitIndex = -1;
    for (let i = targetIndex; i < target.length; i++) {
      if (target[i] === queryChar) {
        hitIndex = i;
        break;
      }
    }
    if (hitIndex < 0) return null;

    score += 1;
    if (previousHitIndex === hitIndex - 1) {
      run += 1;
      score += run; // contiguous bonus
    } else {
      run = 0;
    }
    if (hitIndex === 0 || target[hitIndex - 1] === " ") {
      score += 2; // word-boundary bonus
    }

    previousHitIndex = hitIndex;
    targetIndex = hitIndex + 1;
  }

  return score;
}

function scoreTool(
  query: string,
  tool: ToolDefinition,
): number | null {
  const best = (
    scores: [number | null, number | undefined][]
  ): number | null => {
    let bestScore: number | null = null;
    for (const [score, weight] of scores) {
      if (score === null) continue;
      const weighted = score * (weight ?? 1);
      if (bestScore === null || weighted > bestScore) bestScore = weighted;
    }
    return bestScore;
  };

  const nameScore = fuzzyScore(query, tool.name);
  const categoryScore = fuzzyScore(query, getCategoryLabel(tool.category));
  const keywordScore = best(
    tool.keywords.map(
      (keyword) => [fuzzyScore(query, keyword), undefined] as [number | null, number | undefined],
    ),
  );
  const descriptionScore = fuzzyScore(query, tool.description);

  // Name is the primary identity — weight it strongest.
  const candidates: [number | null, number | undefined][] = [
    [nameScore, 2],
    [keywordScore, undefined],
    [categoryScore, undefined],
    [descriptionScore, undefined],
  ];
  return best(candidates);
}

/**
 * Empty query → every tool, registry order.
 * Non-empty → matches only, best score first; ties keep registry order.
 */
export function searchTools(
  query: string,
  tools: readonly ToolDefinition[] = TOOLS,
): ToolDefinition[] {
  const q = query.trim();
  if (!q) return [...tools];

  const scored = tools
    .map((tool, index) => ({ tool, index, score: scoreTool(q, tool) }))
    .filter((entry): entry is { tool: ToolDefinition; index: number; score: number } => entry.score !== null);

  return scored
    .sort(
      (a, b) =>
        b.score - a.score || a.index - b.index, // stable tie-break by registry order
    )
    .map((entry) => entry.tool);
}
