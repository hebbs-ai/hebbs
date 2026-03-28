# TASK: Make hebbs.ai SEO and AI-crawler friendly

## Problem

AI tools (ChatGPT, Perplexity, etc.) report they cannot scan hebbs.ai. The page is heavily JavaScript-driven with canvas-based visuals (Memory Palace, neural mesh background) and interactive components (code tabs, provider switchers). Crawlers that don't execute JS see a mostly empty page.

## Requirements

1. **Ensure all key text content is server-rendered (SSR/static HTML)**
   - Astro already does this for text sections, but verify every section's headings, paragraphs, and code blocks are in the initial HTML, not hydrated client-side
   - The canvas visuals (Memory Palace, neural mesh) will never be crawlable -- that's fine

2. **Meta tags**
   - Add comprehensive `<meta name="description">` on every page (website, blog, docs, guide)
   - Add Open Graph tags (`og:title`, `og:description`, `og:image`, `og:url`)
   - Add Twitter Card tags
   - Add structured data (JSON-LD) for the website homepage and blog posts

3. **Sitemap**
   - Verify sitemaps are generated and accessible at `/sitemap.xml` for all three sites (hebbs.ai, blog.hebbs.ai, docs.hebbs.ai)

4. **robots.txt**
   - Allow all crawlers on all three sites
   - Point to sitemap

5. **Blog post SEO**
   - Blog already has JSON-LD BlogPosting schema -- verify it's correct
   - Ensure hero images have proper alt text
   - Verify canonical URLs

6. **Performance**
   - Check Core Web Vitals (LCP, CLS, FID) -- heavy canvas animations may impact these
   - Consider lazy-loading the Memory Palace canvas only when it enters viewport (already using IntersectionObserver, verify it's not blocking LCP)

## Files to check

- `hebbs-website/src/layouts/Layout.astro` -- meta tags, structured data
- `hebbs-website/src/pages/index.astro` -- homepage meta
- `hebbs-blog/src/layouts/BlogPost.astro` -- blog post meta/JSON-LD
- `hebbs-docs/astro.config.mjs` -- sitemap plugin
- All three `public/robots.txt` files

## Priority

Medium. Not blocking, but limits discoverability by AI-powered search tools which are increasingly how developers find tools.
