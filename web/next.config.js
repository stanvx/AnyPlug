/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'export',
  // Disable image optimization for static export
  images: { unoptimized: true },
  // Base path for serving behind reverse proxy or from /web subdirectory
  basePath: '',
  // Use relative links so the SPA works from any path
  assetPrefix: '',
  // Use trailing slashes so static export creates dirs with index.html
  trailingSlash: true,
};

module.exports = nextConfig;
