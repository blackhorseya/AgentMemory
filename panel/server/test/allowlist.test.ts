import { describe, expect, it } from "vitest";
import { CORE_V3_ALLOWLIST, resolveV3Path } from "../src/http/allowlist.js";

describe("resolveV3Path", () => {
  it("accepts allowlisted paths", () => {
    expect(resolveV3Path("atomic/query")).toBe("atomic/query");
    expect(resolveV3Path("meta/auth/verify")).toBe("meta/auth/verify");
    expect(resolveV3Path("memory-generation-log/list")).toBe("memory-generation-log/list");
  });

  it("normalises redundant slashes", () => {
    expect(resolveV3Path("atomic//query/")).toBe("atomic/query");
  });

  it.each([
    "instance/destroy",
    "internal/meta/user/init-admin",
    "meta/user/delete",
    "atomic/../instance/destroy",
    "atomic/query%2f..",
    "ATOMIC/QUERY",
    "",
    "/",
  ])("rejects %j", (tail) => {
    expect(resolveV3Path(tail)).toBeUndefined();
  });

  it("never allowlists destructive admin routes", () => {
    for (const path of CORE_V3_ALLOWLIST) {
      expect(path.startsWith("internal/")).toBe(false);
      expect(path.startsWith("instance/")).toBe(false);
    }
  });
});
