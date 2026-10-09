import { readFileSync } from 'node:fs'
import process from 'node:process'

const mean = (values) => values.reduce((sum, value) => sum + value, 0) / values.length
const percentile = (values, fraction) =>
  [...values].sort((a, b) => a - b)[Math.ceil(values.length * fraction) - 1]

function metrics(results) {
  const rows = results.map(({ query, top10 }) => {
    const grades = top10.map(({ id }) => query.relevant[id] ?? 0)
    const first = grades.findIndex((grade) => grade > 0)
    const dcg = (values) =>
      values
        .slice(0, 5)
        .reduce((sum, grade, index) => sum + (2 ** grade - 1) / Math.log2(index + 2), 0)
    const ideal = Object.values(query.relevant).sort((a, b) => b - a)
    return {
      hit1: Number(grades[0] > 0),
      direct1: Number(grades[0] === 2),
      hit5: Number(grades.slice(0, 5).some((grade) => grade > 0)),
      mrr10: first < 0 ? 0 : 1 / (first + 1),
      ndcg5: dcg(grades) / dcg(ideal),
    }
  })
  return {
    count: results.length,
    ...Object.fromEntries(
      Object.keys(rows[0]).map((key) => [key, mean(rows.map((row) => row[key]))]),
    ),
  }
}

if (process.argv.length < 3) throw new Error('Usage: node summarize.mjs RESULT_JSON...')
const summary = process.argv.slice(2).map((file) => {
  const run = JSON.parse(readFileSync(file, 'utf8'))
  const times = run.results.map((result) => result.encode_ms)
  return {
    file,
    dimension: run.dimension,
    overall: metrics(run.results),
    categories: Object.fromEntries(
      [...new Set(run.results.map(({ query }) => query.category))].map((category) => [
        category,
        metrics(run.results.filter(({ query }) => query.category === category)),
      ]),
    ),
    query_ms: { p50: percentile(times, 0.5), p95: percentile(times, 0.95) },
    query_tokens: {
      p50: percentile(
        run.results.map((result) => result.tokens),
        0.5,
      ),
      max: Math.max(...run.results.map((result) => result.tokens)),
    },
    documents_ms: run.document_encode_ms.reduce((sum, ms) => sum + ms, 0),
    load_ms: run.load_ms,
    linux_memory: run.linux_memory,
    misses_at_5: run.results
      .filter(({ query, top10 }) => !top10.slice(0, 5).some(({ id }) => query.relevant[id]))
      .map(({ query }) => query.text),
  }
})
process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`)
