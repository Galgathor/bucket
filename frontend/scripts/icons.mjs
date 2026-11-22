import sharp from "sharp";
const input = "public/icon.svg";
await sharp(input).resize(192).png().toFile("public/icon-192.png");
await sharp(input).resize(512).png().toFile("public/icon-512.png");
await sharp({
  create: { width: 512, height: 512, channels: 4, background: "#6554c0" },
})
  .composite([
    {
      input: await sharp(input).resize(340).png().toBuffer(),
      gravity: "center",
    },
  ])
  .png()
  .toFile("public/icon-maskable.png");
