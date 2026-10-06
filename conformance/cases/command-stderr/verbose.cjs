// Fails after more stderr than the error keeps: a first line, 500 lines of progress, then
// the reason.
const lines = ["FIRST-LINE"];
for (let i = 0; i < 500; i++) lines.push(`progress line ${i} of the noise`);
lines.push("build failed: out of disk space; free some space and re-run");
process.stderr.write(`${lines.join("\n")}\n`);
process.exitCode = 1;
