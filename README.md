# GitHub Deployment Tool

This simple Rust app is used to watch for push events to a repository on GitHub and
build/deploy the software locally. Basically GitHub sends a *push event* notification when
something is pushed to a repository and this app listens for those events. When it receives
an event for its configured repository and branch it executes a local script that can do
whatever it wants. Normally the script could change to the local clone of the repository,
pull the changes, build the thing and optionally deploy it.

This app is kept as simple as possible which means it doesn't do any pulling etc on its
own but that's left entirely to the external script. Failures are not retried, only logged,

## Compiling

```shell
cargo build
```

Copy the created binary somewhere suitable.

## Build script

The service requires a script or application that is executed whenever a push to the monitored
repository is received. This script/app can do whatever it wants, but typically it would:

* change to the repository directory
* `git pull`
* build the software
* install the software
* restart services as needed

## Setting up the webhook

In the repository on GitHub: **Settings → Webhooks → Add webhook**. You need admin access to the repository.

| Field            | Value                                                |
|------------------|------------------------------------------------------|
| Payload URL      | Your public address                                  |
| Content type     | `application/json`                                   |
| Secret           | A secret, use in `WEBHOOK_SECRET`                    |
| SSL verification | Leave enabled if your server has a valid certificate |
| Which events     | "Just the push event"                                |
| Active           | Checked                                              |

## Running

The supplied `github-deployment.service` can be used as a base for a Systemd service.

### User

The service file uses a `deploy` user for the service. Adapt as needed and optionally create
a new user.

### Environment

The service needs a `WEBHOOK_SECRET` environment variable that contains the GitHub webhook secret
as set up on GitHub. One way to set it is to use an environment file that Systemd reads and sets up
when running the app.

```unit file (systemd)
[Service]
EnvironmentFile=/etc/github-deploy/env
```

This file should contain only one line:

```
WEBHOOK_SECRET=abc123
```
