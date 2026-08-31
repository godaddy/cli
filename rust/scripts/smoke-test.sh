#!/usr/bin/env bash
# Smoke test for every settings situation, over the real gddy binary and a
# mocked app-registry-api. Never touches ~/.config/gddy — uses a scratch XDG_CONFIG_HOME.
set -uo pipefail

rust_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$rust_root"

mock_pid=""
scratch_dir=""
failures=0

pass() { echo "PASS: $1"; }
fail() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

cleanup() {
  if [ -n "$mock_pid" ]; then
    kill "$mock_pid" 2>/dev/null
    wait "$mock_pid" 2>/dev/null
  fi
  if [ -n "$scratch_dir" ]; then
    rm -rf "$scratch_dir"
  fi
}
trap cleanup EXIT

echo "==> cargo build"
cargo build --quiet
cargo build --quiet --example smoke_mock_server

echo "==> starting mock app-registry-api"
mock_log="$(mktemp)"
./target/debug/examples/smoke_mock_server >"$mock_log" 2>&1 &
mock_pid=$!
mock_port=""
for _ in $(seq 1 20); do
  mock_port="$(grep -m1 '^PORT=' "$mock_log" 2>/dev/null | cut -d= -f2)"
  [ -n "$mock_port" ] && break
  sleep 0.25
done
if [ -z "$mock_port" ]; then
  fail "mock server never printed its port"
  cat "$mock_log"
  exit 1
fi
echo "    mock listening on 127.0.0.1:$mock_port"

scratch_dir="$(mktemp -d)"
export XDG_CONFIG_HOME="$scratch_dir/.config"
mkdir -p "$XDG_CONFIG_HOME/gddy"
cat >"$XDG_CONFIG_HOME/gddy/environments.toml" <<EOF
[smoke]
api_url = "http://127.0.0.1:$mock_port"
client_id = "00000000-0000-4000-8000-000000000000"
min_stage = "experimental"
EOF

export GDDY_PAT_SMOKE="gd_pat_smoketest0000"

cd "$scratch_dir"
mkdir -p fixtures

gddy() { "$rust_root/target/debug/gddy" --env smoke "$@"; }

base_fields() {
  cat <<'EOF'
name = "smoke-app"
client_id = "3fa85f64-5717-4562-b3fc-2c963f66afa6"
version = "1.0.0"
url = "https://example.com"
proxy_url = "https://example.com/proxy"
authorization_scopes = ["commerce.order:read"]
EOF
}

write_manifest() {
  { base_fields; cat; } >godaddy.smoke.toml
}

check_valid() {
  local desc="$1" out
  out="$(gddy platform app config validate 2>&1)"
  if echo "$out" | jq -e '.data.valid == true' >/dev/null 2>&1; then
    pass "$desc"
  else
    fail "$desc: $out"
  fi
}

check_invalid() {
  local desc="$1" pattern="$2" out
  out="$(gddy platform app config validate 2>&1)"
  if echo "$out" | jq -e '.data.valid == false' >/dev/null 2>&1 && echo "$out" | grep -q "$pattern"; then
    pass "$desc"
  else
    fail "$desc: $out"
  fi
}

echo "=== Group A: local validation (config validate, no network) ==="

write_manifest <<'EOF'
[[settings]]
group = "Tax_Center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
EOF
check_invalid "A1 rejects invalid group slug" "settings\[0\].group must match"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "settings/manual-tax"
EOF
check_invalid "A2 rejects entryPath missing leading slash" "must be a route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
capabilities = ["read", "not-a-capability"]
EOF
check_invalid "A3 rejects an unknown capability" "capabilities contains"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[settings.icon]
name = "percent"
library = "material"
EOF
check_invalid "A4 rejects an unknown icon library" "icon.library must be one of"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "a"
entryPath = "/settings/tax"

[[settings]]
group = "tax-center"
slug = "b"
entryPath = "/settings/tax"
EOF
check_invalid "A5 rejects overlapping entryPaths" "overlaps with"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flagA"
label = "Flag A"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults again"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flagB"
label = "Flag B"
EOF
check_invalid "A6 rejects duplicate section keys" "duplicates another section key"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "s1"
label = "S1"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag"

[[settings.presentation.sections]]
key = "s2"
label = "S2"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag again"
EOF
check_invalid "A7 rejects duplicate field keys across sections" "duplicates another field key"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "s1"
label = "S1"

[[settings.presentation.sections.fields]]
type = "select"
key = "choice"
label = "Choice"
options = []
EOF
check_invalid "A8 rejects a select field with no options" "must contain at least one option"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "s1"
label = "S1"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "1bad"
label = "Bad"
EOF
check_invalid "A9 rejects a malformed field key" "fields\[0\].key must match"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "s1"
label = "S1"

[[settings.presentation.sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"

[settings.presentation.sections.fields.item]
idField = "1bad"

[[settings.presentation.sections.fields.item.fields]]
type = "text"
key = "name"
label = "Name"
EOF
check_invalid "A10 rejects a malformed list-group idField" "item.idField must match"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
presentationFile = "fixtures/manual-tax.json"

[[settings.presentation.sections]]
key = "x"
label = "X"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag"
EOF
check_invalid "A11 rejects presentation + presentationFile on the same entry" "presentationFile"

write_manifest <<'EOF'
EOF
icon_out="$(gddy platform app add settings \
  --group tax-center --slug manual-tax --entry-path /settings/manual-tax \
  --icon-name percent 2>&1)"
if [ $? -ne 0 ] && echo "$icon_out" | grep -q "icon-name and --icon-library must be provided together"; then
  pass "A12 rejects --icon-name without --icon-library"
else
  fail "A12 did not reject a lone --icon-name: $icon_out"
fi

write_manifest <<'EOF'
EOF
icon_out="$(gddy platform app add settings \
  --group tax-center --slug manual-tax --entry-path /settings/manual-tax \
  --icon-library lucide 2>&1)"
if [ $? -ne 0 ] && echo "$icon_out" | grep -q "icon-name and --icon-library must be provided together"; then
  pass "A13 rejects --icon-library without --icon-name"
else
  fail "A13 did not reject a lone --icon-library: $icon_out"
fi

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "Manual_Tax"
entryPath = "/settings/manual-tax"
EOF
check_invalid "A14 rejects invalid slug pattern" "settings\[0\].slug must match"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax?x=1"
EOF
check_invalid "A15 rejects entryPath with a query string" "route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax#frag"
EOF
check_invalid "A16 rejects entryPath with a fragment" "route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "https://example.com/settings"
EOF
check_invalid "A17 rejects entryPath with a scheme" "route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/../manual-tax"
EOF
check_invalid "A18 rejects entryPath with a .. segment" "route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/"
EOF
check_invalid "A19 rejects a bare / entryPath" "route-safe path"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "root"
entryPath = "/settings"

[[settings]]
group = "tax-center"
slug = "child"
entryPath = "/settings/tax"
EOF
check_invalid "A20 rejects entryPath overlap via path prefix" "overlaps with"

write_manifest <<'EOF'
EOF
add_out="$(gddy platform app add settings \
  --group tax-center --slug manual-tax \
  --entry-path /settings/manual-tax \
  --title "GoDaddy Tax" --description "Tax settings" \
  --order 10 \
  --capability read --capability write \
  --icon-name percent --icon-library lucide 2>&1)"
add_status=$?
if [ "$add_status" -eq 0 ] \
  && grep -q 'group = "tax-center"' godaddy.smoke.toml \
  && grep -q 'slug = "manual-tax"' godaddy.smoke.toml \
  && grep -q 'entryPath = "/settings/manual-tax"' godaddy.smoke.toml \
  && grep -q 'title = "GoDaddy Tax"' godaddy.smoke.toml \
  && grep -q 'description = "Tax settings"' godaddy.smoke.toml \
  && grep -q 'order = 10' godaddy.smoke.toml \
  && grep -q '"read"' godaddy.smoke.toml \
  && grep -q '"write"' godaddy.smoke.toml \
  && grep -q 'name = "percent"' godaddy.smoke.toml \
  && grep -q 'library = "lucide"' godaddy.smoke.toml; then
  pass "A21 add settings writes the full flag set to godaddy.toml"
else
  fail "A21 add settings did not write the expected fields (exit $add_status): $add_out"
fi
check_valid "A22 config validate accepts the CLI-added placement-only entry"

write_manifest <<'EOF'
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "write"]

[settings.presentation]
label = "Configure PayPal"
openMode = "new-window"
EOF
check_invalid "A23 rejects settings-link-v1 without exactly read+open capabilities" "requires exactly the read and open capabilities"

write_manifest <<'EOF'
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open"]

[settings.presentation]
label = ""
openMode = "new-window"
EOF
check_invalid "A24 rejects a settings-link-v1 presentation with an empty label" "label must not be empty"

write_manifest <<'EOF'
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open"]

[settings.presentation]
label = "Configure PayPal"
openMode = "same-window"
EOF
check_invalid "A25 rejects a settings-link-v1 presentation with a non-new-window openMode" "openMode must be"

write_manifest <<'EOF'
[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
capabilities = ["read", "write", "open"]

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag"
EOF
check_invalid "A26 rejects the open capability on a settings-form-v1 presentation" "only valid for a settings-link-v1"

write_manifest <<'EOF'
[[settings]]
group = "payment-methods"
slug = "paypal-payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open"]

[settings.presentation]
label = "Configure PayPal"
openMode = "new-window"
EOF
check_valid "A27 config validate accepts a well-formed settings-link-v1 entry"

echo "=== Group B: release-time behavior (mocked network) ==="

cat >fixtures/manual-tax.json <<'EOF'
{
  "type": "form",
  "schemaVersion": "settings-form-v1",
  "sections": [{
    "key": "defaults",
    "label": "Defaults",
    "fields": [
      {"type": "text", "key": "displayName", "label": "Display name", "required": true, "defaultValue": "GoDaddy Tax"},
      {"type": "textarea", "key": "notes", "label": "Notes"},
      {"type": "number", "key": "rate", "label": "Rate", "min": 0, "max": 100, "defaultValue": 7.5},
      {"type": "boolean", "key": "autoCalculate", "label": "Auto-calculate", "defaultValue": true},
      {"type": "select", "key": "calculateUsing", "label": "Calculate using", "defaultValue": "destination",
       "options": [{"value": "destination", "label": "Destination"}, {"value": "origin", "label": "Origin"}]},
      {"type": "multi-select", "key": "regions", "label": "Regions",
       "options": [{"value": "us", "label": "US"}, {"value": "ca", "label": "Canada"}]},
      {"type": "list-group", "key": "rules", "label": "Rules",
       "item": {"idField": "id", "titleField": "country",
                "fields": [{"type": "select", "key": "country", "label": "Country",
                            "options": [{"value": "US", "label": "United States"}]}]}}
    ]
  }]
}
EOF

write_manifest <<'EOF'

[[settings]]
group = "tax-center"
slug = "manual-tax"
title = "GoDaddy Tax"
description = "Tax settings"
entryPath = "/settings/manual-tax"
order = 10
capabilities = ["read", "write", "validate"]
presentationFile = "fixtures/manual-tax.json"

[settings.icon]
name = "percent"
library = "lucide"

[[settings]]
group = "tax-center"
slug = "manual-tax-inline"
entryPath = "/settings/manual-tax-inline"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "text"
key = "displayName"
label = "Display name"
required = true
defaultValue = "GoDaddy Tax"

[[settings.presentation.sections.fields]]
type = "textarea"
key = "notes"
label = "Notes"

[[settings.presentation.sections.fields]]
type = "number"
key = "rate"
label = "Rate"
min = 0.0
max = 100.0
defaultValue = 7.5

[[settings.presentation.sections.fields]]
type = "boolean"
key = "autoCalculate"
label = "Auto-calculate"
defaultValue = true

[[settings.presentation.sections.fields]]
type = "select"
key = "calculateUsing"
label = "Calculate using"
defaultValue = "destination"

[[settings.presentation.sections.fields.options]]
value = "destination"
label = "Destination"

[[settings.presentation.sections.fields.options]]
value = "origin"
label = "Origin"

[[settings.presentation.sections.fields]]
type = "multi-select"
key = "regions"
label = "Regions"

[[settings.presentation.sections.fields.options]]
value = "us"
label = "US"

[[settings.presentation.sections.fields.options]]
value = "ca"
label = "Canada"

[[settings.presentation.sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"

[settings.presentation.sections.fields.item]
idField = "id"
titleField = "country"

[[settings.presentation.sections.fields.item.fields]]
type = "select"
key = "country"
label = "Country"

[[settings.presentation.sections.fields.item.fields.options]]
value = "US"
label = "United States"
EOF

check_valid "B1 config validate accepts both entries, fixture unopened"

cp fixtures/manual-tax.json fixtures/manual-tax.json.bak

rm fixtures/manual-tax.json
out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "could not be read" && echo "$out" | grep -q "manual-tax.json"; then
  pass "B2 release fails when presentationFile is missing"
else
  fail "B2 did not fail as expected on a missing presentationFile (exit $status): $out"
fi

echo "not json" >fixtures/manual-tax.json
out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "is invalid" && echo "$out" | grep -q "manual-tax.json"; then
  pass "B3 release fails on malformed JSON in presentationFile"
else
  fail "B3 did not fail as expected on malformed presentationFile (exit $status): $out"
fi

jq '.schemaVersion = "something-else"' fixtures/manual-tax.json.bak >fixtures/manual-tax.json
out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q 'schemaVersion must be'; then
  pass "B4 release fails on the wrong schemaVersion in presentationFile"
else
  fail "B4 did not fail as expected on a wrong schemaVersion (exit $status): $out"
fi

mv fixtures/manual-tax.json.bak fixtures/manual-tax.json

echo "$(base_fields)

[[settings]]
group = \"tax-center\"
slug = \"no-presentation\"
entryPath = \"/settings/no-presentation\"" >godaddy.smoke.toml
out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "no presentation"; then
  pass "B5 release fails when a setting has neither presentation nor presentationFile"
else
  fail "B5 did not fail as expected on a placement-only entry (exit $status): $out"
fi

write_manifest <<'EOF'

[[settings]]
group = "tax-center"
slug = "manual-tax"
title = "GoDaddy Tax"
description = "Tax settings"
entryPath = "/settings/manual-tax"
order = 10
capabilities = ["read", "write", "validate"]
presentationFile = "fixtures/manual-tax.json"

[settings.icon]
name = "percent"
library = "lucide"

[[settings]]
group = "tax-center"
slug = "manual-tax-inline"
entryPath = "/settings/manual-tax-inline"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "text"
key = "displayName"
label = "Display name"
required = true
defaultValue = "GoDaddy Tax"

[[settings.presentation.sections.fields]]
type = "textarea"
key = "notes"
label = "Notes"

[[settings.presentation.sections.fields]]
type = "number"
key = "rate"
label = "Rate"
min = 0.0
max = 100.0
defaultValue = 7.5

[[settings.presentation.sections.fields]]
type = "boolean"
key = "autoCalculate"
label = "Auto-calculate"
defaultValue = true

[[settings.presentation.sections.fields]]
type = "select"
key = "calculateUsing"
label = "Calculate using"
defaultValue = "destination"

[[settings.presentation.sections.fields.options]]
value = "destination"
label = "Destination"

[[settings.presentation.sections.fields.options]]
value = "origin"
label = "Origin"

[[settings.presentation.sections.fields]]
type = "multi-select"
key = "regions"
label = "Regions"

[[settings.presentation.sections.fields.options]]
value = "us"
label = "US"

[[settings.presentation.sections.fields.options]]
value = "ca"
label = "Canada"

[[settings.presentation.sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"

[settings.presentation.sections.fields.item]
idField = "id"
titleField = "country"

[[settings.presentation.sections.fields.item.fields]]
type = "select"
key = "country"
label = "Country"

[[settings.presentation.sections.fields.item.fields.options]]
value = "US"
label = "United States"
EOF

release_out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
if echo "$release_out" | jq -e '.data.settings | length == 2' >/dev/null 2>&1; then
  file_presentation="$(echo "$release_out" | jq -c '.data.settings[] | select(.appSettingSlug=="manual-tax") | .presentation')"
  inline_presentation="$(echo "$release_out" | jq -c '.data.settings[] | select(.appSettingSlug=="manual-tax-inline") | .presentation')"
  if [ "$file_presentation" = "$inline_presentation" ] && [ -n "$file_presentation" ]; then
    pass "B6 presentationFile and inline presentation are identical across all field types"
  else
    fail "B6 presentation payloads differ — file: $file_presentation inline: $inline_presentation"
  fi
  file_entry="$(echo "$release_out" | jq -c '.data.settings[] | select(.appSettingSlug=="manual-tax")')"
  if echo "$file_entry" | jq -e '
      .title == "GoDaddy Tax" and .description == "Tax settings" and .order == 10
      and .capabilities == ["read","write","validate"]
      and .iconName == "percent" and .iconLibrary == "lucide"
    ' >/dev/null 2>&1; then
    pass "B7 release echoes title/description/order/capabilities/icon"
  else
    fail "B7 release did not echo optional fields correctly: $file_entry"
  fi
else
  fail "B6/B7 release did not succeed with both settings entries: $release_out"
fi

cp godaddy.smoke.toml godaddy.smoke.toml.bak
echo "this = is not [valid toml" >godaddy.smoke.toml
out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "failed to load"; then
  pass "B8 release fails on a manifest that fails to parse"
else
  fail "B8 did not fail on an unparseable manifest (exit $status): $out"
fi
mv godaddy.smoke.toml.bak godaddy.smoke.toml

echo "=== Group C: additional settings coverage ==="

write_manifest <<'EOF'

[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"
metadata = { internalId = "abc123", tags = ["a", "b"] }

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag"
EOF
release_out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
if echo "$release_out" | jq -e '
    .data.settings[0].metadata.internalId == "abc123"
    and .data.settings[0].metadata.tags == ["a","b"]
  ' >/dev/null 2>&1; then
  pass "C1 release echoes arbitrary settings metadata"
else
  fail "C1 metadata not echoed as expected: $release_out"
fi

write_manifest <<'EOF'
EOF
release_out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
if echo "$release_out" | jq -e '.data.settings == []' >/dev/null 2>&1; then
  pass "C2 release succeeds with zero settings entries"
else
  fail "C2 release with no settings did not return an empty settings array: $release_out"
fi

write_manifest <<'EOF'

[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "autoCalculate"
label = "Auto-calculate"
defaultValue = true

[[settings.presentation.sections]]
key = "advanced"
label = "Advanced"

[settings.presentation.sections.visibleWhen]
field = "autoCalculate"
equals = true

[[settings.presentation.sections.fields]]
type = "multi-select"
key = "regions"
label = "Regions"
minItems = 1
maxItems = 2

[[settings.presentation.sections.fields.options]]
value = "us"
label = "US"

[[settings.presentation.sections.fields.options]]
value = "ca"
label = "Canada"

[[settings.presentation.sections.fields]]
type = "list-group"
key = "rules"
label = "Rules"
minItems = 0
maxItems = 5

[settings.presentation.sections.fields.item]
idField = "id"

[[settings.presentation.sections.fields.item.fields]]
type = "select"
key = "priority"
label = "Priority"
defaultValue = 1

[[settings.presentation.sections.fields.item.fields.options]]
value = 1
label = "Low"

[[settings.presentation.sections.fields.item.fields.options]]
value = 2
label = "High"
EOF
check_valid "C3a config validate accepts minItems/maxItems, visibleWhen, and numeric option values"

release_out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
if echo "$release_out" | jq -e '
    .data.settings[0].presentation.sections[1].visibleWhen == {"field":"autoCalculate","equals":true}
    and .data.settings[0].presentation.sections[1].fields[0].minItems == 1
    and .data.settings[0].presentation.sections[1].fields[0].maxItems == 2
    and .data.settings[0].presentation.sections[1].fields[1].minItems == 0
    and .data.settings[0].presentation.sections[1].fields[1].maxItems == 5
    and .data.settings[0].presentation.sections[1].fields[1].item.fields[0].options[0].value == 1
  ' >/dev/null 2>&1; then
  pass "C3b release echoes visibleWhen/minItems/maxItems/numeric option values unchanged"
else
  fail "C3b release did not echo extended field options as expected: $release_out"
fi

write_manifest <<'EOF'

[[settings]]
group = "payment-methods"
slug = "paypal-payments"
title = "PayPal Payments"
entryPath = "/settings/paypal"
capabilities = ["read", "open"]

[settings.presentation]
label = "Configure PayPal"
openMode = "new-window"
EOF
release_out="$(gddy platform app release --application-id smoke-app-id --version 0.0.1 2>&1)"
if echo "$release_out" | jq -e '
    .data.settings[0].presentation.type == "link"
    and .data.settings[0].presentation.schemaVersion == "settings-link-v1"
    and .data.settings[0].presentation.label == "Configure PayPal"
    and .data.settings[0].presentation.openMode == "new-window"
    and .data.settings[0].capabilities == ["read","open"]
  ' >/dev/null 2>&1; then
  pass "C4 release echoes a settings-link-v1 entry with type/schemaVersion/label/openMode"
else
  fail "C4 release did not echo the link entry as expected: $release_out"
fi

echo "=== Group D: server error responses (mocked network) ==="

write_manifest <<'EOF'

[[settings]]
group = "tax-center"
slug = "manual-tax"
entryPath = "/settings/manual-tax"

[[settings.presentation.sections]]
key = "defaults"
label = "Defaults"

[[settings.presentation.sections.fields]]
type = "boolean"
key = "flag"
label = "Flag"
EOF

out="$(gddy platform app release --application-id smoke-http-500-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "HTTP error 500"; then
  pass "D1 release surfaces a 500 response from the server"
else
  fail "D1 did not surface a 500 as expected (exit $status): $out"
fi

out="$(gddy platform app release --application-id smoke-http-401-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "HTTP error 401"; then
  pass "D2 release surfaces a 401 response from the server"
else
  fail "D2 did not surface a 401 as expected (exit $status): $out"
fi

out="$(gddy platform app release --application-id smoke-graphql-error-id --version 0.0.1 2>&1)"
status=$?
if [ "$status" -ne 0 ] && echo "$out" | grep -q "release not found"; then
  pass "D3 release surfaces a GraphQL errors array on an HTTP 200"
else
  fail "D3 did not surface a GraphQL error as expected (exit $status): $out"
fi

echo
if [ "$failures" -eq 0 ]; then
  echo "==> smoke test passed"
  exit 0
else
  echo "==> smoke test FAILED ($failures check(s))"
  exit 1
fi
