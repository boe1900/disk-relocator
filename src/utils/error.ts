function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseJsonIfPossible(input: string): unknown {
  const trimmed = input.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
    return input;
  }
  try {
    return JSON.parse(trimmed);
  } catch {
    return input;
  }
}

function normalizeError(err: unknown): unknown {
  if (typeof err === "string") {
    return parseJsonIfPossible(err);
  }
  return err;
}

export function formatCommandError(err: unknown): string {
  const normalized = normalizeError(err);

  if (normalized instanceof Error) {
    return normalized.message || normalized.name;
  }

  if (typeof normalized === "string") {
    return normalized;
  }

  if (isRecord(normalized)) {
    const code = typeof normalized.code === "string" ? normalized.code : null;
    const message = typeof normalized.message === "string" ? normalized.message : null;
    const traceId = typeof normalized.trace_id === "string" ? normalized.trace_id : null;
    const details = isRecord(normalized.details) ? normalized.details : null;
    const detailError = details && typeof details.error === "string" ? details.error : null;

    const parts: string[] = [];
    if (code) {
      parts.push(code);
    }
    if (message) {
      parts.push(message);
    }
    if (detailError && detailError !== message) {
      parts.push(detailError);
    }
    if (traceId) {
      parts.push(`trace_id=${traceId}`);
    }

    if (parts.length > 0) {
      return parts.join(" | ");
    }

    try {
      return JSON.stringify(normalized);
    } catch {
      return "[unknown error]";
    }
  }

  return String(normalized);
}
