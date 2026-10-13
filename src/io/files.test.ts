// Lane 16 (C6): a file is measured before it is read. A dropped or chosen file over its
// limit is refused by its size, without its bytes ever being read.
import { describe, expect, it, vi } from "vitest";
import { checkSize, FileTooLarge, MAX_READ_BYTES, readBrowserFile, readLimit } from "./files";

/** A File that says it is `size` bytes and fails the test if it is read. */
function claimed(name: string, size: number) {
  const read = vi.fn(async () => new ArrayBuffer(0));
  return { file: { name, size, arrayBuffer: read } as unknown as File, read };
}

describe("reading files", () => {
  it("each kind of file has its limit", () => {
    expect(readLimit("Bench.shz")).toBe(MAX_READ_BYTES.shz);
    expect(readLimit("plugin.zip")).toBe(MAX_READ_BYTES.zip);
    expect(readLimit("chair.skp")).toBe(MAX_READ_BYTES.import);
    expect(readLimit("part.STL")).toBe(MAX_READ_BYTES.import);
  });

  it("a file over its limit is refused by its size, never read", async () => {
    const { file, read } = claimed("huge.stl", MAX_READ_BYTES.import + 1);
    await expect(readBrowserFile(file)).rejects.toBeInstanceOf(FileTooLarge);
    expect(read).not.toHaveBeenCalled();
    const doc = claimed("huge.shz", 3 * 1024 * 1024 * 1024);
    await expect(readBrowserFile(doc.file)).rejects.toThrow("Sawhorse reads documents of at most 512 MB");
    expect(doc.read).not.toHaveBeenCalled();
  });

  it("a file within its limit is read", async () => {
    const { file, read } = claimed("ok.obj", 1024);
    const r = await readBrowserFile(file);
    expect(read).toHaveBeenCalledOnce();
    expect(r.name).toBe("ok.obj");
  });

  it("checkSize names the file, its size and the limit", () => {
    expect(() => checkSize("C/models/big.zip", 100 * 1024 * 1024)).toThrow("big.zip is 100 MB; Sawhorse reads plugin packages of at most 64 MB");
    expect(() => checkSize("small.zip", 1024)).not.toThrow();
  });
});
