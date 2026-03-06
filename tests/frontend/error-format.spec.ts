import { describe, expect, it } from "vitest";
import { formatCommandError } from "../../src/utils/error";

describe("formatCommandError", () => {
  it("formats plain Error instances", () => {
    expect(formatCommandError(new Error("disk offline"))).toBe("disk offline");
  });

  it("formats command error objects with details and trace id", () => {
    const text = formatCommandError({
      code: "MIGRATE_COPY_FAILED",
      message: "failed during copy source -> temp.",
      trace_id: "tr_abc123",
      details: {
        error: "Operation not permitted (os error 1)"
      }
    });

    expect(text).toContain("MIGRATE_COPY_FAILED");
    expect(text).toContain("failed during copy source -> temp.");
    expect(text).toContain("Operation not permitted (os error 1)");
    expect(text).toContain("trace_id=tr_abc123");
  });

  it("parses JSON-stringified errors from invoke", () => {
    const text = formatCommandError(
      JSON.stringify({
        code: "PRECHECK_DISK_OFFLINE",
        message: "target disk is offline or not mounted.",
        trace_id: "tr_offline_1"
      })
    );

    expect(text).toContain("PRECHECK_DISK_OFFLINE");
    expect(text).toContain("target disk is offline or not mounted.");
    expect(text).toContain("trace_id=tr_offline_1");
  });
});
