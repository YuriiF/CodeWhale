import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { buildPageMetadata, IDENTITY_PHRASE, SITE_NAME, SITE_URL } from "./page-meta";

describe("page metadata", () => {
  it.each([
    ["en", "/faq", "FAQ · Codewhale"],
    ["zh", "/faq", "常见问题 · Codewhale"],
    ["en", "/feed", "Activity · Codewhale"],
    ["zh", "/feed", "动态 · Codewhale"],
    ["en", "/roadmap", "Roadmap · Codewhale"],
    ["zh", "/roadmap", "路线图 · Codewhale"],
  ])("builds canonical, hreflang, Open Graph, and Twitter fields for %s%s", (locale, path, title) => {
    const description = `${locale} metadata contract`;
    const metadata = buildPageMetadata({ path, locale, title, description });
    const canonical = `${SITE_URL}/${locale}${path}`;

    expect(metadata.alternates).toEqual({
      canonical,
      languages: {
        en: `${SITE_URL}/en${path}`,
        zh: `${SITE_URL}/zh${path}`,
        "x-default": `${SITE_URL}/en${path}`,
      },
    });
    expect(metadata.openGraph).toEqual({
      title,
      description,
      url: canonical,
      siteName: SITE_NAME,
      type: "website",
      locale: locale === "zh" ? "zh_CN" : "en_US",
      images: [
        {
          url: `${SITE_URL}/opengraph-image`,
          width: 1200,
          height: 630,
          alt: `${SITE_NAME} — ${IDENTITY_PHRASE}`,
        },
      ],
    });
    expect(metadata.twitter).toEqual({
      card: "summary_large_image",
      title,
      description,
      images: [`${SITE_URL}/opengraph-image`],
    });
  });

  it("keeps the previously incomplete indexable routes on the shared helper", () => {
    for (const [route, path] of [
      ["faq", "/faq"],
      ["feed", "/feed"],
      ["roadmap", "/roadmap"],
    ]) {
      const source = readFileSync(
        new URL(`../app/[locale]/${route}/page.tsx`, import.meta.url),
        "utf8",
      );
      expect(source, route).toContain('import { buildPageMetadata } from "@/lib/page-meta"');
      expect(source, route).toContain("return buildPageMetadata({");
      expect(source, route).toContain(`path: "${path}"`);
    }
  });
});
