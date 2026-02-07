const entries: Array<{ entry: string; outfile: string }> = [
  { entry: "./src/preload-app.ts", outfile: "./dist/preload-app.js" },
  {
    entry: "./src/preload-wallet-selector.ts",
    outfile: "./dist/preload-wallet-selector.js",
  },
  { entry: "./src/preload-tabbar.ts", outfile: "./dist/preload-tabbar.js" },
  { entry: "./src/home.tsx", outfile: "./dist/home.js" },
  { entry: "./src/launcher.tsx", outfile: "./dist/launcher.js" },
  { entry: "./src/wallet-selector.tsx", outfile: "./dist/wallet-selector.js" },
  { entry: "./src/tabbar.tsx", outfile: "./dist/tabbar.js" },
];

await Bun.$`mkdir -p ./dist`;

for (const { entry, outfile } of entries) {
  const proc = Bun.spawn(
    [
      "bun",
      "build",
      entry,
      "--bundle",
      "--production",
      "--target=browser",
      "--format=iife",
      "--minify",
      "--define",
      `process.env.NODE_ENV='"production"'`,
      "--outfile",
      outfile,
    ],
    {
      stdout: "inherit",
      stderr: "inherit",
    }
  );

  const code = await proc.exited;
  if (code !== 0) {
    process.exit(code);
  }
}
