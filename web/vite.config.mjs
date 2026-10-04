import { defineConfig } from 'vite';

export default defineConfig({
  optimizeDeps: { entries: ['index.html'] },
  server: {
    watch: { ignored: ['**/.local/**', '**/media-src/**', '**/dist/**', '**/.vercel/**'] },
  },
  plugins: [{
    name: 'inline-landing-styles',
    apply: 'build',
    transformIndexHtml: {
      order: 'post',
      handler(html, context) {
        return html.replace(/<link\b[^>]*rel="stylesheet"[^>]*>/g, tag => {
          const href = tag.match(/href="([^"]+)"/)?.[1];
          const asset = href && context.bundle?.[href.replace(/^\//, '')];
          if (!asset || asset.type !== 'asset' || typeof asset.source !== 'string') return tag;
          return `<style>${asset.source.replace(/<\/style/gi, '<\\/style')}</style>`;
        });
      },
    },
  }],
});
