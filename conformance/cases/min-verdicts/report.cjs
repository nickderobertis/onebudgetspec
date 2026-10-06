// Writes its first argument, verbatim, as the measurement's result.
const fs = require("node:fs");

fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, process.argv[2]);
