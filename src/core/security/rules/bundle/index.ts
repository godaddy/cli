/**
 * Bundle security rules (SEC101-SEC110).
 * Exported as array for scanner consumption.
 */
export { SEC101_EVAL } from "./SEC101-eval.ts";
export { SEC102_CHILD_PROCESS } from "./SEC102-child-process.ts";
export { SEC103_VM } from "./SEC103-vm.ts";
export { SEC104_PROCESS_BINDING } from "./SEC104-process-binding.ts";
export { SEC105_NATIVE_ADDON } from "./SEC105-native-addon.ts";
export { SEC106_MODULE_PATCH } from "./SEC106-module-patch.ts";
export { SEC107_INSPECTOR } from "./SEC107-inspector.ts";
export { SEC108_EXTERNAL_URL } from "./SEC108-external-url.ts";
export { SEC109_ENCODED_BLOB } from "./SEC109-encoded-blob.ts";
export { SEC110_SENSITIVE_OPS } from "./SEC110-sensitive-ops.ts";

import { SEC101_EVAL } from "./SEC101-eval.ts";
import { SEC102_CHILD_PROCESS } from "./SEC102-child-process.ts";
import { SEC103_VM } from "./SEC103-vm.ts";
import { SEC104_PROCESS_BINDING } from "./SEC104-process-binding.ts";
import { SEC105_NATIVE_ADDON } from "./SEC105-native-addon.ts";
import { SEC106_MODULE_PATCH } from "./SEC106-module-patch.ts";
import { SEC107_INSPECTOR } from "./SEC107-inspector.ts";
import { SEC108_EXTERNAL_URL } from "./SEC108-external-url.ts";
import { SEC109_ENCODED_BLOB } from "./SEC109-encoded-blob.ts";
import { SEC110_SENSITIVE_OPS } from "./SEC110-sensitive-ops.ts";

import type { BundleRule } from "../../types.ts";

export const BUNDLE_RULES: BundleRule[] = [
	SEC101_EVAL,
	SEC102_CHILD_PROCESS,
	SEC103_VM,
	SEC104_PROCESS_BINDING,
	SEC105_NATIVE_ADDON,
	SEC106_MODULE_PATCH,
	SEC107_INSPECTOR,
	SEC108_EXTERNAL_URL,
	SEC109_ENCODED_BLOB,
	SEC110_SENSITIVE_OPS,
];
