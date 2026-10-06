// Succeeds, warning on stderr, which no report shows.
const fs = require("node:fs");

process.stderr.write("warning: cache cold\n");
fs.writeFileSync(process.env.ONEBUDGETSPEC_RESULT, '{"value": 3}');
