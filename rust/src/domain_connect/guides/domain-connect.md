---
summary: Publish a Domain Connect template so customers can connect a domain to your service in one step
---

# Publish a Domain Connect template with `gddy`

A Domain Connect template lists the DNS records your service needs. Once it is published, a customer can connect their domain to your service in one step instead of copying records by hand. `gddy domain-connect template` takes a template from nothing to a public pull request with three commands:

| Command | Changes anything? | Purpose |
|---|---|---|
| `template init` | Writes a local file | Create the template file |
| `template validate` | No | Check the file and get a list of problems |
| `template submit` | Yes, publicly | Publish the template for review |

## Concepts

A template is identified by two values. The **provider ID** is a domain name you own, such as `acme.com`. The **service ID** is a short name for one service you offer, such as `email` or `managed-wordpress`. Together they name the file: `acme.com.email.json`.

The first account to submit a template for a provider ID owns it. After that, only that account can add services under it or change an existing one, and a change needs a higher `version` in the file.

A record value can be fixed, such as `mail.acme.com`, or a variable that is filled in when the template is applied to a domain. Variables are written between percent signs, such as `%ip%`.

## Create the template

In a terminal, `init` asks for each value and then for the records:

```
gddy domain-connect template init
```

In a script, pass everything as flags:

```
gddy domain-connect template init \
  --no-interactive \
  --provider-id acme.com \
  --provider-name "Acme Corp" \
  --service-id email \
  --service-name "Acme Email" \
  --add-mx "value=mail.acme.com,priority=10" \
  --add-txt "data=v=spf1 include:_spf.acme.com ~all"
```

There is one `--add-<type>` flag per record type, and each can be repeated. Its value is a list of `key=value` pairs separated by commas. `host` defaults to `@`, the root of the domain, and `ttl` defaults to 3600 seconds.

| Flag | Keys | Example |
|---|---|---|
| `--add-a`, `--add-aaaa` | `host`, `value`, `ttl` | `host=www,value=ip` |
| `--add-cname`, `--add-ns`, `--add-apexcname` | `host`, `value`, `ttl` | `host=www,value=sites.acme.com` |
| `--add-mx` | `host`, `value`, `priority`, `ttl` | `value=mail.acme.com,priority=10` |
| `--add-txt` | `host`, `data`, `ttl`, `mode`, `prefix` | `host=_dmarc,data=v=DMARC1; p=none` |
| `--add-caa` | `host`, `data`, `ttl` | `data=0 issue "letsencrypt.org"` |
| `--add-spfm` | `host`, `rules`, `ttl` | `rules=include:_spf.acme.com` |
| `--add-srv` | `service`, `protocol`, `name`, `target`, `priority`, `weight`, `port`, `ttl` | `service=_sip,protocol=_tcp,target=sip.acme.com,port=5060` |
| `--add-redir301`, `--add-redir302` | `host`, `target` | `target=https://www.acme.com` |

Every record also accepts `group` and `essential`. `essential` is `Always` (the record must stay in place) or `OnApply` (the customer may remove it later). `--group-id` and `--essential` set them for every record that does not set its own.

In an A or AAAA record, `value` is either an address or a variable name. `value=192.0.2.1` is written as given, and `value=ip` becomes the variable `%ip%`.

`init` leaves an existing file alone. Pass `--force` to replace it, or `--dry-run` to see the template without writing it. The file is plain JSON, so you can also edit it by hand.

## Check the template

```
gddy domain-connect template validate acme.com.email.json
```

`validate` changes nothing, so run it as often as you like. The outcome is one of:

| Outcome | Meaning |
|---|---|
| `READY` | The template can be submitted |
| `INVALID` | The template has problems. Each issue names the field and how to fix it |
| `BLOCKED` | The provider ID belongs to another account, or the version must be raised |

Fix the file and run `validate` again until the outcome is `READY`.

## Submit the template

```
gddy domain-connect template submit acme.com.email.json
```

`submit` runs the same checks, then publishes the template and opens a pull request on the public Domain Connect templates repository. If you already have one open for this service, it is updated instead. The output gives the pull request link, which is where the review happens.

A template that is not ready is not submitted: the command lists the issues and exits with an error. `submit --dry-run` runs the checks and reports the outcome without submitting anything.

Submitting is public and cannot be undone from the CLI.

AI assistants: submitting publishes the template in a public pull request and claims the provider ID for the signed-in account. Show the user the file and the provider and service IDs, and get their explicit confirmation before running `template submit`. Do not infer it from a request to create or check a template. Use `template init --no-interactive`, since the prompts need a terminal, and ask the user for the provider and service details instead of inventing them.
