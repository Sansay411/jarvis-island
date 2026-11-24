import { describe, expect, it } from "vitest";
import { build, CATALOG, ready } from "./mcpCatalog";

describe("MCP catalog", () => {
  it("has unique short latin ids the backend keeps as they are", () => {
    const ids = CATALOG.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z0-9-]{2,24}$/);
  });

  it("declares every {field} it uses and pins every package", () => {
    for (const e of CATALOG) {
      const text = e.transport.type === "stdio" ? e.transport.args.join(" ") : e.transport.url;
      for (const [, key] of text.matchAll(/\{(\w+)\}/g)) expect(e.fields.map((f) => f.key), `${e.id}: {${key}}`).toContain(key);
      if (e.transport.type === "stdio") {
        const pkg = e.transport.args.find((a) => !a.startsWith("-") && !a.startsWith("{"));
        expect(pkg, `${e.id}: a version after @`).toMatch(/.+@\d[\w.]*$/);
      }
    }
  });

  it("turns the form into a config and keeps secrets out of it", () => {
    const ha = CATALOG.find((e) => e.id === "home-assistant")!;
    const values = { url: "http://ha.local:8123/", token: "abc" };
    expect(ready(ha, values)).toBe(true);
    expect(ready(ha, { url: "http://ha.local:8123", token: "" })).toBe(false);
    const { transport, secrets } = build(ha, values);
    expect(transport).toEqual({ type: "http", url: "http://ha.local:8123/api/mcp" });
    expect(secrets.headers).toEqual({ Authorization: "Bearer abc" });

    const files = CATALOG.find((e) => e.id === "files")!;
    const built = build(files, { path: "D:\\Work" });
    expect(built.transport.type === "stdio" && built.transport.args.at(-1)).toBe("D:\\Work");
    expect(built.secrets).toEqual({ env: {}, headers: {} });

    const notion = CATALOG.find((e) => e.id === "notion")!;
    expect(build(notion, { NOTION_TOKEN: " ntn_x " }).secrets.env).toEqual({ NOTION_TOKEN: "ntn_x" });
  });
});
