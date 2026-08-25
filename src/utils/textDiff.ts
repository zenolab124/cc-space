import { diffLines, diffWordsWithSpace, type Change } from 'diff'

export type TextDiffRowKind = 'context' | 'add' | 'delete'
export type TextDiffChunkKind = 'context' | 'add' | 'delete'

export interface TextDiffChunk {
  kind: TextDiffChunkKind
  text: string
}

export interface TextDiffRow {
  kind: TextDiffRowKind
  oldNumber: number | null
  newNumber: number | null
  text: string
  chunks?: TextDiffChunk[]
}

export interface TextDiffStats {
  additions: number
  deletions: number
}

export interface TextDiffModel {
  rows: TextDiffRow[]
  stats: TextDiffStats
}

export type FoldedTextDiffRow = TextDiffRow | {
  kind: 'fold'
  hiddenCount: number
}

function changeLines(value: string): string[] {
  if (!value) return []
  const lines = value.split('\n')
  if (lines[lines.length - 1] === '') lines.pop()
  return lines
}

function inlineChunks(oldText: string, newText: string): {
  oldChunks: TextDiffChunk[]
  newChunks: TextDiffChunk[]
} {
  const changes = diffWordsWithSpace(oldText, newText)
  return {
    oldChunks: changes
      .filter(change => !change.added)
      .map(change => ({ kind: change.removed ? 'delete' : 'context', text: change.value })),
    newChunks: changes
      .filter(change => !change.removed)
      .map(change => ({ kind: change.added ? 'add' : 'context', text: change.value })),
  }
}

/** 为相邻的删除/新增行配对词级差异；多余行仍保留整行强调。 */
function annotateInlineChanges(rows: TextDiffRow[]): TextDiffRow[] {
  const annotated = rows.map(row => ({ ...row }))
  let index = 0
  while (index < annotated.length) {
    if (annotated[index].kind !== 'delete') {
      index += 1
      continue
    }
    const deletedStart = index
    while (index < annotated.length && annotated[index].kind === 'delete') index += 1
    const addedStart = index
    while (index < annotated.length && annotated[index].kind === 'add') index += 1
    const paired = Math.min(addedStart - deletedStart, index - addedStart)
    for (let offset = 0; offset < paired; offset += 1) {
      const deleted = annotated[deletedStart + offset]
      const added = annotated[addedStart + offset]
      const chunks = inlineChunks(deleted.text, added.text)
      deleted.chunks = chunks.oldChunks
      added.chunks = chunks.newChunks
    }
  }
  return annotated
}

function rowsFromTexts(oldText: string, newText: string): TextDiffRow[] {
  const rows: TextDiffRow[] = []
  let oldNumber = 1
  let newNumber = 1
  for (const change of diffLines(oldText, newText) as Change[]) {
    const kind: TextDiffRowKind = change.added
      ? 'add'
      : change.removed
        ? 'delete'
        : 'context'
    for (const text of changeLines(change.value)) {
      rows.push({
        kind,
        oldNumber: kind === 'add' ? null : oldNumber++,
        newNumber: kind === 'delete' ? null : newNumber++,
        text,
      })
    }
  }
  return annotateInlineChanges(rows)
}

function rowsFromUnifiedDiff(diff: string): TextDiffRow[] {
  const rows: TextDiffRow[] = []
  let oldNumber = 1
  let newNumber = 1
  let insideHunk = false
  const lines = diff.split('\n')

  for (const [index, line] of lines.entries()) {
    if (index === lines.length - 1 && line === '') continue
    const hunk = line.match(/^@@\s+-(\d+)(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@/)
    if (hunk) {
      oldNumber = Number(hunk[1])
      newNumber = Number(hunk[2])
      insideHunk = true
      continue
    }
    if (
      line.startsWith('diff --git ')
      || line.startsWith('index ')
      || line.startsWith('--- ')
      || line.startsWith('+++ ')
      || line === '\\ No newline at end of file'
    ) continue

    const marker = line[0]
    if (marker === '+') {
      rows.push({ kind: 'add', oldNumber: null, newNumber: newNumber++, text: line.slice(1) })
    } else if (marker === '-') {
      rows.push({ kind: 'delete', oldNumber: oldNumber++, newNumber: null, text: line.slice(1) })
    } else if (marker === ' ' || insideHunk) {
      rows.push({
        kind: 'context',
        oldNumber: oldNumber++,
        newNumber: newNumber++,
        text: marker === ' ' ? line.slice(1) : line,
      })
    }
  }
  return annotateInlineChanges(rows)
}

function statsOf(rows: readonly TextDiffRow[]): TextDiffStats {
  return {
    additions: rows.filter(row => row.kind === 'add').length,
    deletions: rows.filter(row => row.kind === 'delete').length,
  }
}

export function calculateTextDiffStats(options: {
  oldText?: string | null
  newText?: string | null
  unifiedDiff?: string | null
}): TextDiffStats {
  const unifiedDiff = options.unifiedDiff ?? ''
  if (unifiedDiff.trim()) {
    let additions = 0
    let deletions = 0
    for (const line of unifiedDiff.split('\n')) {
      if (line.startsWith('+++ ') || line.startsWith('--- ')) continue
      if (line.startsWith('+')) additions += 1
      else if (line.startsWith('-')) deletions += 1
    }
    return { additions, deletions }
  }

  let additions = 0
  let deletions = 0
  for (const change of diffLines(options.oldText ?? '', options.newText ?? '') as Change[]) {
    const count = changeLines(change.value).length
    if (change.added) additions += count
    else if (change.removed) deletions += count
  }
  return { additions, deletions }
}

export function buildTextDiff(options: {
  oldText?: string | null
  newText?: string | null
  unifiedDiff?: string | null
}): TextDiffModel {
  const unifiedDiff = options.unifiedDiff ?? ''
  const rows = unifiedDiff.trim()
    ? rowsFromUnifiedDiff(unifiedDiff)
    : rowsFromTexts(options.oldText ?? '', options.newText ?? '')
  return { rows, stats: statsOf(rows) }
}

/** 折叠长未修改区，只保留每个变更两侧的少量上下文。 */
export function foldTextDiffRows(
  rows: readonly TextDiffRow[],
  contextLines = 3,
): FoldedTextDiffRow[] {
  const folded: FoldedTextDiffRow[] = []
  let index = 0
  while (index < rows.length) {
    if (rows[index].kind !== 'context') {
      folded.push(rows[index])
      index += 1
      continue
    }
    const start = index
    while (index < rows.length && rows[index].kind === 'context') index += 1
    const run = rows.slice(start, index)
    const keepBefore = start === 0 ? 0 : Math.min(contextLines, run.length)
    const keepAfter = index === rows.length ? 0 : Math.min(contextLines, run.length - keepBefore)
    const hiddenCount = run.length - keepBefore - keepAfter
    if (keepBefore) folded.push(...run.slice(0, keepBefore))
    if (hiddenCount > 0) folded.push({ kind: 'fold', hiddenCount })
    if (keepAfter) folded.push(...run.slice(run.length - keepAfter))
  }
  return folded
}
