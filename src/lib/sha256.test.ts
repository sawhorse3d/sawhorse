import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import { sha256Hex } from "./sha256";

describe("sha256Hex", () => {
  it("matches the FIPS 180-4 examples", () => {
    expect(sha256Hex("")).toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    expect(sha256Hex("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    expect(sha256Hex("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")).toBe("248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1");
  });
  it("matches Node's crypto at every length round the block boundaries, and on UTF-8 text", () => {
    for (let n = 0; n < 200; n++) {
      const bytes = new Uint8Array(n).map((_, i) => (i * 31 + n) & 255);
      expect(sha256Hex(bytes), `length ${n}`).toBe(createHash("sha256").update(bytes).digest("hex"));
    }
    const text = "Bücherregal \u{1F528} \"quoted\" \\ back";
    expect(sha256Hex(text)).toBe(createHash("sha256").update(text, "utf8").digest("hex"));
  });
});
