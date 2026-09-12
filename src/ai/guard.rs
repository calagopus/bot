use std::sync::LazyLock;

/// Why a drafted answer was held back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block {
    /// A command that destroys data or weakens the security of a machine.
    Destructive(&'static str),
    /// The answer asks someone to post a secret into a public thread.
    Credentials,
    /// A command the documentation does not back, so nothing checked it.
    UnsourcedCommand,
}

impl Block {
    pub fn reason(self) -> String {
        match self {
            Self::Destructive(rule) => format!("destructive command ({rule})"),
            Self::Credentials => "asks for credentials".to_string(),
            Self::UnsourcedCommand => "command without a verified source".to_string(),
        }
    }
}

/// Strips `$ `, `# ` and a shell prompt such as `root@node:~#` from the front of a line.
static PROMPT: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"^\s*(?:[\w.-]+@[\w.-]+:[^#$]*)?[#$]\s+")
        .expect("invalid ai guard prompt pattern")
});

fn strip_prompt(line: &str) -> String {
    PROMPT.replace(line.trim(), "").trim().to_string()
}

fn code_regions(answer: &str) -> Vec<String> {
    let mut regions = Vec::new();
    let mut fence: Option<&str> = None;

    for line in answer.lines() {
        let trimmed = line.trim();

        if let Some(marker) = fence {
            if trimmed.starts_with(marker) {
                fence = None;
            } else {
                regions.push(strip_prompt(line));
            }

            continue;
        }

        if let Some(marker) = ["```", "~~~"]
            .into_iter()
            .find(|marker| trimmed.starts_with(marker))
        {
            let rest = &trimmed[marker.len()..];

            match rest.strip_suffix(marker) {
                Some(inner) => regions.push(strip_prompt(inner)),
                None => fence = Some(marker),
            }

            continue;
        }

        if line.starts_with("    ") || line.starts_with('\t') {
            regions.push(strip_prompt(line));

            continue;
        }

        if trimmed.matches('`').count() % 2 == 0 {
            for span in trimmed.split('`').skip(1).step_by(2) {
                regions.push(strip_prompt(span));
            }
        }
    }

    regions
}

fn commands(answer: &str) -> Vec<String> {
    code_regions(answer)
        .iter()
        .flat_map(|region| {
            region
                .replace("&&", "\n")
                .replace("||", "\n")
                .replace(';', "\n")
                .split('\n')
                .map(|command| command.split_whitespace().collect::<Vec<_>>().join(" "))
                .map(|command| command.to_lowercase())
                .collect::<Vec<_>>()
        })
        .filter(|command| !command.is_empty())
        .collect()
}

fn head(command: &str) -> Option<&str> {
    command
        .split_whitespace()
        .find(|token| *token != "sudo" && !token.contains('='))
}

const COMMAND_WORDS: [&str; 40] = [
    "rm",
    "mv",
    "cp",
    "chmod",
    "chown",
    "chgrp",
    "dd",
    "mkfs",
    "mount",
    "umount",
    "docker",
    "docker-compose",
    "podman",
    "systemctl",
    "journalctl",
    "service",
    "apt",
    "apt-get",
    "yum",
    "dnf",
    "pacman",
    "curl",
    "wget",
    "mysql",
    "mysqldump",
    "psql",
    "pg_dump",
    "redis-cli",
    "iptables",
    "ufw",
    "firewall-cmd",
    "setenforce",
    "sed",
    "tar",
    "unzip",
    "openssl",
    "certbot",
    "crontab",
    "bash",
    "sh",
];

fn is_command(command: &str) -> bool {
    let tokens = command.split_whitespace().collect::<Vec<_>>();

    let named = head(command).is_some_and(|head| {
        COMMAND_WORDS.contains(&head) || head.starts_with("./") || head.starts_with('/')
    });

    named
        && (tokens.len() >= 3
            || tokens.iter().any(|token| token.starts_with('-'))
            || tokens.iter().skip(1).any(|token| token.contains('/')))
}

const READ_ONLY: [&str; 34] = [
    "ls",
    "cat",
    "less",
    "more",
    "tail",
    "head",
    "grep",
    "wc",
    "stat",
    "file",
    "df",
    "du",
    "free",
    "uptime",
    "uname",
    "whoami",
    "id",
    "ps",
    "top",
    "htop",
    "ping",
    "dig",
    "nslookup",
    "host",
    "ss",
    "netstat",
    "traceroute",
    "journalctl",
    "dmesg",
    "lsblk",
    "lsof",
    "nproc",
    "getenforce",
    "tree",
];

const DOCKER_READ_ONLY: [&str; 12] = [
    "logs", "ps", "inspect", "images", "version", "info", "stats", "top", "port", "diff", "events",
    "config",
];

const SYSTEMCTL_READ_ONLY: [&str; 6] = [
    "status",
    "is-active",
    "is-enabled",
    "is-failed",
    "list-units",
    "show",
];

fn is_read_only(command: &str) -> bool {
    let tokens = command
        .split_whitespace()
        .filter(|token| *token != "sudo" && !token.contains('='))
        .collect::<Vec<_>>();

    let Some(head) = tokens.first() else {
        return false;
    };

    let subcommand = tokens
        .iter()
        .skip(1)
        .find(|token| !token.starts_with('-') && **token != "compose");

    match *head {
        "docker" | "docker-compose" | "podman" => {
            subcommand.is_some_and(|subcommand| DOCKER_READ_ONLY.contains(subcommand))
        }
        "systemctl" => {
            subcommand.is_some_and(|subcommand| SYSTEMCTL_READ_ONLY.contains(subcommand))
        }
        "ufw" | "firewall-cmd" => subcommand
            .is_some_and(|subcommand| *subcommand == "status" || *subcommand == "--list-all"),
        "iptables" => tokens
            .iter()
            .any(|token| *token == "-l" || *token == "-s" || *token == "--list"),
        head => READ_ONLY.contains(&head),
    }
}

const PROTECTED_PATHS: [&str; 22] = [
    "/",
    "~",
    "$home",
    ".",
    "..",
    "/etc",
    "/var",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/boot",
    "/opt",
    "/srv",
    "/root",
    "/home",
    "/dev",
    "/proc",
    "/sys",
    "/var/lib/docker",
    "/var/lib/calagopus",
    "/home/container",
];

fn destroys_a_protected_path(command: &str) -> bool {
    let mut tokens = command.split_whitespace().peekable();

    if tokens.peek() == Some(&"sudo") {
        tokens.next();
    }

    if tokens.next() != Some("rm") {
        return false;
    }

    let mut recursive = false;
    let mut force = false;
    let mut targets = Vec::new();

    for token in tokens {
        if token == "--no-preserve-root" {
            return true;
        }

        if let Some(flag) = token.strip_prefix("--") {
            match flag {
                "recursive" => recursive = true,
                "force" => force = true,
                _ => {}
            }

            continue;
        }

        if let Some(flags) = token.strip_prefix('-') {
            recursive |= flags.contains('r') || flags.contains('R');
            force |= flags.contains('f');

            continue;
        }

        targets.push(token);
    }

    if !recursive || !force {
        return false;
    }

    targets.iter().any(|target| {
        let target = match *target {
            "/" => "/",
            target => target.trim_end_matches('/'),
        };

        PROTECTED_PATHS.contains(&target)
            || target.ends_with("/*")
            || target.starts_with("~/")
            || PROTECTED_PATHS
                .iter()
                .any(|protected| *protected == target.trim_end_matches("/*"))
    })
}

static DESTRUCTIVE: LazyLock<Vec<(&'static str, regex::Regex)>> = LazyLock::new(|| {
    let patterns = [
        ("filesystem format", r"^(sudo )?mkfs(\.[a-z0-9]+)?\b"),
        ("raw write to a block device", r"\bdd\b.*\bof=/dev/"),
        ("redirect over a block device", r">\s*/dev/(sd|nvme|vd|hd)"),
        // Pruning and volume removal take every server's data with them.
        ("docker prune", r"docker (system|volume) prune"),
        (
            "docker prune",
            r"docker (image|container) prune\b.*(-a\b|--all\b)",
        ),
        ("docker volume removal", r"docker volume (rm|remove)\b"),
        (
            "bulk container removal",
            r"docker (rm|stop|kill) .*\$\(docker ps",
        ),
        ("privileged container", r"docker run\b.*--privileged"),
        ("database drop", r"\bdrop (database|schema|table)\b"),
        ("table truncation", r"\btruncate table\b"),
        (
            "unbounded delete",
            r"\bdelete from \S+\s*(;|$)|\bdelete from \S+ where 1\s*=\s*1",
        ),
        // Permission and ownership resets that leave a host wide open.
        (
            "world-writable permissions",
            r"\bchmod\b.*\b(0?777|a\+rwx)\b",
        ),
        (
            "ownership reset of a system path",
            r"\bchown\b.*\s(/|/(etc|var|usr|home|root)(/\*)?)(\s|$)",
        ),
        // Handing a downloaded script to a shell.
        (
            "piping a download into a shell",
            r"(curl|wget)\b[^|]*\|\s*(sudo\s+)?(\w+=\S+\s+)*((ba|z|k)?sh|su|python3?|perl|ruby)\b",
        ),
        // Turning off the protections around a panel.
        ("firewall flush", r"\biptables\b.*(\s-f\b|--flush\b)"),
        ("firewall opened", r"\biptables .*-p (input|forward) accept"),
        (
            "firewall disabled",
            r"\bufw (--force )?disable\b|systemctl (stop|disable) (--now )?(ufw|firewalld)|service (ufw|firewalld) stop",
        ),
        (
            "selinux disabled",
            r"\bsetenforce (0|permissive)\b|selinux\s*=\s*disabled",
        ),
        (
            "certificate verification disabled",
            r"(curl|wget)\b[^|]*(\s-k\b|--insecure\b|--no-check-certificate\b)|insecure[_-]?skip[_-]?verify\s*[:=]\s*true|verify[_-]?(ssl|peer|certificate)\s*[:=]\s*(false|off|0)",
        ),
        (
            "remote root login enabled",
            r"permitrootlogin\s+yes|passwordauthentication\s+yes",
        ),
    ];

    patterns
        .into_iter()
        .map(|(rule, pattern)| {
            (
                rule,
                regex::Regex::new(pattern).expect("invalid ai guard pattern"),
            )
        })
        .collect()
});

static CREDENTIAL_REQUEST: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(
        r"\b(send|share|post|paste|give|show|dm)\b[^.]{0,60}\b(password|api[ _-]?key|token|secret|private[ _-]?key|credentials)\b",
    )
    .expect("invalid ai guard credentials pattern")
});

static CREDENTIAL_DESTINATION: LazyLock<regex::Regex> = LazyLock::new(|| {
    regex::Regex::new(r"\b(to )?(me|us)\b|\bhere\b|\bthis (thread|channel|post|message)\b")
        .expect("invalid ai guard destination pattern")
});

const NEGATIONS: [&str; 8] = [
    "never",
    "don't",
    "do not",
    "avoid",
    "shouldn't",
    "should not",
    "must not",
    "no need",
];

fn requests_credentials(answer: &str) -> bool {
    answer
        .split(['.', '!', '?', '\n'])
        .map(|sentence| {
            sentence
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase()
                .replace(['\u{2019}', '\u{02bc}', '\u{2018}'], "'")
        })
        .any(|sentence| {
            !NEGATIONS.iter().any(|negation| sentence.contains(negation))
                && CREDENTIAL_REQUEST.is_match(&sentence)
                && CREDENTIAL_DESTINATION.is_match(&sentence)
        })
}

pub fn check(answer: &str, sourced: bool) -> Option<Block> {
    if code_regions(answer).iter().any(|region| {
        region
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
            .contains(":(){:|:&};:")
    }) {
        return Some(Block::Destructive("fork bomb"));
    }

    let commands = commands(answer);

    for command in &commands {
        if destroys_a_protected_path(command) {
            return Some(Block::Destructive("recursive delete of a protected path"));
        }

        for (rule, pattern) in DESTRUCTIVE.iter() {
            if pattern.is_match(command) {
                return Some(Block::Destructive(rule));
            }
        }
    }

    if requests_credentials(answer) {
        return Some(Block::Credentials);
    }

    if !sourced
        && commands
            .iter()
            .any(|command| is_command(command) && !is_read_only(command))
    {
        return Some(Block::UnsourcedCommand);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn allowed(answer: &str, sourced: bool) {
        assert_eq!(check(answer, sourced), None, "wrongly blocked: {answer}");
    }

    #[track_caller]
    fn blocked(answer: &str, rule: &str) {
        match check(answer, true) {
            Some(Block::Destructive(matched)) => {
                assert_eq!(matched, rule, "blocked by the wrong rule: {answer}")
            }
            other => panic!("wrongly allowed ({other:?}): {answer}"),
        }
    }

    #[test]
    fn allows_ordinary_support_answers() {
        allowed(
            "Restart the daemon with `systemctl restart wings` and read `journalctl -u wings -n 50`.",
            true,
        );
        allowed(
            "Your node is probably unreachable rather than offline. Check that the FQDN resolves.",
            false,
        );
        allowed("Run `docker logs <container>` to see why it exited.", true);
        allowed(
            "Remove that one server's files with `rm -rf /var/lib/calagopus/volumes/<uuid>`.",
            true,
        );
        allowed("Open the port with `ufw allow 8080/tcp`.", true);
        allowed(
            "Fix ownership with `chown -R calagopus:calagopus /var/lib/calagopus/volumes/<uuid>`.",
            true,
        );
    }

    /// Cases an earlier version of this guard blocked by reading the whole answer as one line.
    #[test]
    fn allows_answers_that_only_look_dangerous() {
        allowed("Rotate it with `truncate -s 0 /var/log/wings.log`.", true);
        allowed(
            "Check `curl -I https://panel.example.com`, then `journalctl -k -n 100` for OOM kills.",
            true,
        );
        allowed("Never post your API key in this channel.", true);
        allowed("Do not share your client secret with anyone.", true);
        allowed(
            "Paste the client secret into the panel's OAuth settings.",
            true,
        );
        allowed("In the panel, delete from nodes.", true);
        allowed("Make sure the `docker daemon` is running.", false);
        allowed("The wings service is managed by systemctl.", false);
        allowed("Open the `config.yml in /etc/calagopus.", false);
    }

    #[test]
    fn blocks_destructive_commands() {
        blocked(
            "Just run `rm -rf /`.",
            "recursive delete of a protected path",
        );
        blocked(
            "Run `rm -rf /var/lib/docker` and restart the daemon.",
            "recursive delete of a protected path",
        );
        blocked(
            "Clear them with `rm -rf /var/lib/calagopus/volumes/*`.",
            "recursive delete of a protected path",
        );
        blocked(
            "Try `rm -rf --no-preserve-root /`.",
            "recursive delete of a protected path",
        );
        blocked(
            "Fix it with `chmod -R 0777 /var/lib/calagopus`.",
            "world-writable permissions",
        );
        blocked("Free space with `docker volume prune -f`.", "docker prune");
        blocked("Run `docker system prune --volumes -f`.", "docker prune");
        blocked("Try `ufw --force disable`.", "firewall disabled");
        blocked("Run `systemctl disable --now ufw`.", "firewall disabled");
        blocked("Run `service ufw stop`.", "firewall disabled");
        blocked("Flush them with `iptables -t nat -F`.", "firewall flush");
        blocked("Run `setenforce Permissive`.", "selinux disabled");
        blocked("Run `DELETE FROM servers WHERE 1=1;`", "unbounded delete");
        blocked("Reset it using `DROP DATABASE panel;`", "database drop");
        blocked(
            "Install it with `curl https://example.com/i.sh | sudo bash`.",
            "piping a download into a shell",
        );
        blocked(
            "Run `curl -sSL https://get.docker.com/ | CHANNEL=stable bash`.",
            "piping a download into a shell",
        );
        blocked(
            "Disable the check with `curl -k https://panel.example.com/api`.",
            "certificate verification disabled",
        );
        blocked(
            "Wipe it: `dd if=/dev/zero of=/dev/sda`",
            "raw write to a block device",
        );
        blocked("Run `:(){ :|:& };:` to test it.", "fork bomb");
        blocked(
            "Set `PermitRootLogin yes` in sshd_config.",
            "remote root login enabled",
        );
    }

    /// An answer that warns against a destructive command is held back like one that gives it.
    /// Reading the negation would mean "don't forget to run ..." walks straight through, and a
    /// warning that reaches staff instead of the thread costs little.
    #[test]
    fn blocks_warnings_about_destructive_commands() {
        blocked(
            "Do not run `chmod -R 777 /var/lib/calagopus`, use 750.",
            "world-writable permissions",
        );
    }

    /// The exact draft that was wrongly held back in testing: correct advice, curly apostrophe.
    #[test]
    fn allows_warnings_against_sharing_credentials() {
        allowed(
            "Copy the values into Wings\u{2019} config. Don\u{2019}t post the token here.",
            true,
        );
        allowed("Don\u{2019}t share your API key with anyone here.", true);
        allowed(
            "Post the exact error lines. Do not include credentials or tokens.",
            true,
        );
    }

    #[test]
    fn blocks_credential_requests() {
        assert_eq!(
            check("Please send me your panel API token so I can check.", true),
            Some(Block::Credentials)
        );
        assert_eq!(
            check("Post your database password here and I will look.", true),
            Some(Block::Credentials)
        );
    }

    #[test]
    fn blocks_commands_without_a_source() {
        let answer = "Run `systemctl restart wings` and it should come back.";

        assert_eq!(check(answer, false), Some(Block::UnsourcedCommand));
        allowed(answer, true);
    }

    /// Reading state is not something the docs have to bless first.
    #[test]
    fn allows_read_only_commands_without_a_source() {
        allowed("Check the logs with `docker compose logs -f panel`.", false);
        allowed("Run `docker logs <container> --tail 100`.", false);
        allowed("See what is running: `docker compose ps`.", false);
        allowed("Read them with `journalctl -u wings -n 100`.", false);
        allowed("Check the service with `systemctl status wings`.", false);
        allowed("Is the disk full? `df -h`", false);
        allowed("List them with `ls -la /var/lib/calagopus`.", false);

        // Changing something still needs the documentation behind it.
        assert_eq!(
            check("Run `docker compose up -d` to bring it back.", false),
            Some(Block::UnsourcedCommand)
        );
        assert_eq!(
            check("Run `docker compose restart panel`.", false),
            Some(Block::UnsourcedCommand)
        );
    }

    /// The markdown shapes an earlier version of this guard walked straight past.
    #[test]
    fn finds_commands_in_every_code_shape() {
        let cases = [
            "Run it:\n\n    sudo systemctl restart wings\n",
            "```\ncd /var/lib/x && rm -rf ./volumes\n```",
            "```\nroot@node:~# systemctl restart wings\n```",
            "Start it:\n```docker compose up -d```\nThen fix it up.",
            "```bash\ndocker compose up -d\n```",
            "~~~\nsystemctl restart wings\n~~~",
        ];

        for case in cases {
            assert_eq!(
                check(case, false),
                Some(Block::UnsourcedCommand),
                "command not found in: {case}"
            );
        }
    }

    /// The fence desync that used to hide a command from both layers.
    #[test]
    fn survives_a_one_line_fence_before_a_block() {
        let answer = "Start it:\n```docker compose up -d```\nThen fix the perms:\n```\nsudo chmod -R 0777 /var/lib/calagopus\n```";

        assert_eq!(
            check(answer, true),
            Some(Block::Destructive("world-writable permissions"))
        );
    }
}
