// Plugin storage (lane 15): any key a plugin picks is its own, Object.prototype's names too.
import { afterEach, describe, expect, it } from "vitest";
import { AppStore, DocStore } from "./store";

afterEach(() => new AppStore("org.example.keys").clear());

describe("plugin storage keys", () => {
  for (const make of [() => new AppStore("org.example.keys"), () => new DocStore()] as const) {
    it(`${make().constructor.name}: "constructor", "toString", "hasOwnProperty" and "__proto__" are keys like any other`, () => {
      const s = make();
      const get = (k: string) => (s instanceof AppStore ? s.get(k) : s.get("org.example.keys", k));
      const set = (k: string, v: unknown) => (s instanceof AppStore ? s.set(k, v) : s.set("org.example.keys", k, v));
      const keys = () => (s instanceof AppStore ? s.keys() : s.keys("org.example.keys"));
      for (const k of ["constructor", "toString", "hasOwnProperty", "__proto__"]) expect(get(k), k).toBeNull();
      set("constructor", 1);
      set("toString", "two");
      set("__proto__", { three: 3 });
      expect(get("constructor")).toBe(1);
      expect(get("toString")).toBe("two");
      expect(get("__proto__")).toEqual({ three: 3 });
      expect(get("three")).toBeNull();
      expect(keys().sort()).toEqual(["__proto__", "constructor", "toString"]);
      if (s instanceof DocStore) {
        // Saved with the document and read back the same.
        const again = new DocStore();
        again.load(s.snapshot());
        expect(again.get("org.example.keys", "__proto__")).toEqual({ three: 3 });
        expect(again.get("org.example.keys", "constructor")).toBe(1);
      }
    });
  }
});
