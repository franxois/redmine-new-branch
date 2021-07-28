# redmine-new-branch

A personal tool to create new branch based on our corp redmine

This command :

- use a token to access our redmine api
- download and parse json to read ticket information
- create a git new branch based on origin/master, maintenance branch or parent branch based on ticket information

## how to

    # dev
    cargo watch -x test -x 'build --release'

    # build
    cargo build --release
