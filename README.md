# Introduction

This DDNS tool is to check you current external public IP address and update your domain DNS settings on CloudFlare accordingly.

# Usage

Put a `input.json` file together with the executable.
```json
{
  "token": "{your CloudFlare API token}",
  "zone_id": "{your CloudFlare zone id}",
  "domains": [
    "{your domain 1}",
    "{your domain 2}",
  ]
}

```

```bash
./ddns
```