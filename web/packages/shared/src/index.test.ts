import { describe, expect, it } from "vitest";
import { cn, isErrorCode } from "./index.js";

describe("cn", () => {
  it("拼接真值 class 并过滤假值", () => {
    expect(cn("a", false && "b", undefined, "c")).toBe("a c");
  });
});

describe("isErrorCode", () => {
  it("匹配 <域>-<3位> 错误码", () => {
    expect(isErrorCode("GIT-404")).toBe(true);
    expect(isErrorCode("git-404")).toBe(false);
    expect(isErrorCode("GIT-40")).toBe(false);
  });
});
