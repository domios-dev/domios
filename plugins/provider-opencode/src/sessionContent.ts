import { safeJson, OpencodePart } from "./helpers";

export interface SessionContentQuery {
  maxBytes?: number;
  hintTerms?: string[];
}

interface ContentRow {
  ts?: number;
  part?: string;
  msg?: string;
}

// The budget CTE reads only ids and byte lengths, so rows past the newest
// `maxBytes` are never loaded; hint LIKEs then run on the bounded set.
const CONTENT_SQL_HEAD =
  "WITH budget AS (" +
  "SELECT id, SUM(octet_length(data)) OVER (ORDER BY time_created DESC, id DESC) AS running " +
  "FROM part WHERE session_id = ?1) " +
  "SELECT p.time_created AS ts, p.data AS part, m.data AS msg " +
  "FROM budget b JOIN part p ON p.id = b.id JOIN message m ON m.id = p.message_id " +
  "WHERE b.running <= ?2";
const CONTENT_SQL_TAIL = " ORDER BY p.time_created ASC, p.id ASC";
const TOTAL_SQL = "SELECT SUM(octet_length(data)) AS total FROM part WHERE session_id = ?1";

function queryRows<T>(db: string, sql: string, params: unknown[]): T[] | null {
  const raw = host.sqliteQueryAllJson(db, sql, JSON.stringify(params));
  if (raw === null || raw === undefined) return null;
  const rows = safeJson(raw);
  return Array.isArray(rows) ? (rows as T[]) : null;
}

function likePattern(term: string): string {
  return "%" + term.replace(/[\\%_]/g, "\\$&") + "%";
}

export function readOpencodeSessionContent(
  db: string,
  sessionId: string,
  query: SessionContentQuery | null | undefined,
): unknown {
  const maxBytes = Math.max(0, Math.floor(Number(query && query.maxBytes) || 0));
  const hints = query && Array.isArray(query.hintTerms)
    ? query.hintTerms.filter((term) => typeof term === "string" && term.length > 0)
    : [];
  const totals = queryRows<{ total?: number | null }>(db, TOTAL_SQL, [sessionId]);
  if (!totals) return null;
  const total = Number(totals.length > 0 ? totals[0].total : 0) || 0;

  const params: unknown[] = [sessionId, maxBytes];
  let hintSql = "";
  for (let i = 0; i < hints.length; i++) {
    params.push(likePattern(hints[i]));
    hintSql += " AND p.data LIKE ?" + String(params.length) + " ESCAPE '\\'";
  }
  const rows = queryRows<ContentRow>(db, CONTENT_SQL_HEAD + hintSql + CONTENT_SQL_TAIL, params);
  if (!rows) return null;

  const messages: unknown[] = [];
  for (let i = 0; i < rows.length; i++) {
    const part = safeJson(rows[i].part) as (OpencodePart & { synthetic?: boolean; ignored?: boolean }) | null;
    if (!part || part.type !== "text" || typeof part.text !== "string" || !part.text.trim()) continue;
    const msg = safeJson(rows[i].msg) as { role?: string } | null;
    const ts = rows[i].ts;
    messages.push({
      role: msg && typeof msg.role === "string" ? msg.role : "assistant",
      text: part.text,
      ts: typeof ts === "number" ? Math.floor(ts / 1000) : null,
      isMeta: part.synthetic === true || part.ignored === true,
    });
  }
  return { messages: messages, truncated: total > maxBytes };
}
