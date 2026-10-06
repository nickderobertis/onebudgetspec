// Writes its first argument, verbatim, as the measurement's result.
const fs = require("node:fs");

try {
  fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, process.argv[2]);
} catch {
  process.stderr.write(
    `report.cjs: cannot write the result to '${process.env.ONEBUDGETSPEC_RESULT}'; run this only as a budget's command under 'onebudgetspec check', which creates a writable result file\n`,
  );
  process.exitCode = 1;
}
