use std::time::Duration;

pub(super) const NOTIFICATION_TIMEOUT: Duration = Duration::from_secs(6);
pub(super) const ACTIVE_POLL_INTERVAL: Duration = Duration::from_millis(100);
pub(super) const UI_TIMER_INTERVAL: Duration = Duration::from_millis(500);
pub(super) const IDLE_POLL_INTERVAL: Duration = Duration::from_secs(2);

pub(super) const DEMO_ENTRIES: &[&str] = &[
    "Development/github.com.gpg",
    "Development/linear.app.state.gpg",
    "Development/gitlab.com.gpg",
    "Development/docker.com.gpg",
    "Development/npmjs.com.gpg",
    "Development/crates.io.gpg",
    "Development/aws.console.gpg",
    "Development/cloudflare.com.gpg",
    "Development/sentry.io.gpg",
    "Development/vercel.com.gpg",
    "Email/proton.me.gpg",
    "Email/gmail.com.gpg",
    "Email/fastmail.com.gpg",
    "Email/hey.com.gpg",
    "Email/mailgun.com.gpg",
    "Email/sendgrid.com.gpg",
    "Email/postmark.app.gpg",
    "Email/tutanota.com.gpg",
    "Finance/banking.com.gpg",
    "Finance/wise.com.gpg",
    "Finance/stripe.com.gpg",
    "Finance/coinbase.com.gpg",
    "Finance/fidelity.com.gpg",
    "Finance/taxes.gov.gpg",
    "Personal/airbnb.com.gpg",
    "Personal/figma.com.gpg",
    "Personal/notion.so.gpg",
    "Personal/calendar.google.com.gpg",
    "Personal/photos.google.com.gpg",
    "Personal/spotify.com.gpg",
    "Personal/netflix.com.gpg",
    "Personal/amazon.com.gpg",
    "Personal/homeassistant.local.gpg",
    "Personal/recipes.gpg",
    "Servers/atlas.gpg",
    "Servers/backup.gpg",
    "Servers/edge.gpg",
    "Servers/monitoring.gpg",
    "Servers/postgres.gpg",
    "Servers/redis.gpg",
    "Servers/staging.gpg",
    "Servers/production.gpg",
    "Servers/vpn.gpg",
    "Servers/bastion.gpg",
    "Services/slack.com.gpg",
    "Services/discord.com.gpg",
    "Services/todoist.com.gpg",
    "Services/zoom.us.gpg",
    "Services/dropbox.com.gpg",
    "Services/tailscale.com.gpg",
];

pub(super) const DEMO_RECIPIENT: &str = "DEMO-RECIPIENT";

pub(super) const DEMO_SECRET: &str = "demo-only-not-a-real-password\nusername: alex@example.test\n\nA fictional entry for exploring the interface.\n";
