# Packaging Guide

This guide walks you through setting up automated Linux distribution packaging
for siomon. When a new version tag is pushed, GitHub Actions will automatically
build and publish packages to the [AUR](https://aur.archlinux.org/) (Arch
Linux) and a [Launchpad PPA](https://launchpad.net/) (Ubuntu/Debian). Both
workflows can also be triggered manually.

## Table of Contents

- [Overview](#overview)
- [Prerequisites](#prerequisites)
- [Part 1: Create a Packaging Identity](#part-1-create-a-packaging-identity)
  - [1.1 Generate a GPG Key](#11-generate-a-gpg-key)
  - [1.2 Generate SSH Keys](#12-generate-ssh-keys)
- [Part 2: Set Up AUR Publishing](#part-2-set-up-aur-publishing)
  - [2.1 Create an AUR Account](#21-create-an-aur-account)
  - [2.2 Register Your SSH Key](#22-register-your-ssh-key)
  - [2.3 Register the Package on AUR](#23-register-the-package-on-aur)
- [Part 3: Set Up Launchpad PPA Publishing](#part-3-set-up-launchpad-ppa-publishing)
  - [3.1 Create a Launchpad Account](#31-create-a-launchpad-account)
  - [3.2 Register Your GPG Key on Launchpad](#32-register-your-gpg-key-on-launchpad)
  - [3.3 Register Your SSH Key on Launchpad](#33-register-your-ssh-key-on-launchpad)
  - [3.4 Create a PPA](#34-create-a-ppa)
- [Part 4: Configure GitHub Secrets](#part-4-configure-github-secrets)
  - [4.1 Required Secrets](#41-required-secrets)
  - [4.2 How to Add Secrets](#42-how-to-add-secrets)
  - [4.3 Obtaining Secret Values](#43-obtaining-secret-values)
- [Part 5: Using the Workflows](#part-5-using-the-workflows)
  - [5.1 Automatic Publishing (Tag Push)](#51-automatic-publishing-tag-push)
  - [5.2 Manual AUR Publishing](#52-manual-aur-publishing)
  - [5.3 Manual PPA Publishing](#53-manual-ppa-publishing)
- [Part 6: Troubleshooting](#part-6-troubleshooting)
  - [GPG Keyserver Propagation](#gpg-keyserver-propagation)

---

## Overview

The release pipeline works as follows:

1. You push a version tag (e.g., `v0.3.0`) to the repository.
2. The existing **Release** workflow builds binaries, publishes to crates.io,
   and creates a GitHub Release.
3. After the GitHub Release is created, two additional workflows run in
   parallel:
   - **Publish to AUR** — builds and validates the PKGBUILD in an Arch Linux
     container, then pushes to the AUR via git over SSH. Automatically
     increments `pkgrel` when re-publishing the same upstream version.
   - **Publish to PPA** — vendors dependencies, builds signed Debian source
     packages for each Ubuntu series, and uploads them to a Launchpad PPA via
     SFTP. Automatically increments the repack suffix (`+dsN`) for re-runs
     of the same version to ensure a unique, uploadable version.

Both workflows can also be triggered manually from the GitHub Actions UI.

## Prerequisites

You will need:

- A machine with `gpg` and `ssh-keygen` installed (any Linux distro or macOS).
- A GitHub account with write access to this repository.
- An email address to use as the packaging identity.

---

## Part 1: Create a Packaging Identity

All publishing uses a single identity. You need a GPG key (for signing PPA
packages) and two SSH keys (one for AUR, one for Launchpad).

### 1.1 Generate a GPG Key

The GPG key is used to sign Ubuntu/Debian source packages before uploading to
Launchpad. Launchpad verifies the signature against the GPG key registered on
your account.

1. **Generate the key:**

   ```bash
   gpg --full-generate-key
   ```

   When prompted:
   - **Key type:** Choose `(1) RSA and RSA` (or the default).
   - **Key size:** `4096` bits.
   - **Expiration:** Choose an appropriate expiry or `0` for no expiry.
   - **Real name:** Your packaging identity name (e.g.,
     `Level1Techs Package Team`).
   - **Email:** The email for this identity (e.g.,
     `level1techspackageteam@gmail.com`).
   - **Passphrase:** Choose a passphrase or leave empty for no passphrase.
     If you set a passphrase, you will need to store it as a GitHub secret
     later. If you leave it empty, the `PKG_GPG_PASSPHRASE` secret can be
     omitted.

2. **Verify the key was created:**

   ```bash
   gpg --list-secret-keys --keyid-format long
   ```

   You will see output like:

   ```
   sec   rsa4096/ABCDEF1234567890 2026-01-01 [SC]
         1234567890ABCDEF1234567890ABCDEF12345678
   uid                 [ultimate] Level1Techs Package Team <level1techspackageteam@gmail.com>
   ssb   rsa4096/0987654321FEDCBA 2026-01-01 [E]
   ```

   The long hex string on the second line is your **key fingerprint**. The
   email address is your **key ID** (used in the `PKG_GPG_KEY_ID` secret).

3. **Upload the key to Ubuntu's keyserver** (required for Launchpad):

   ```bash
   gpg --keyserver keyserver.ubuntu.com --send-keys YOUR_KEY_FINGERPRINT
   ```

   Replace `YOUR_KEY_FINGERPRINT` with the full fingerprint from step 2.

   It can take a few minutes for the key to propagate. You can verify it
   arrived with:

   ```bash
   gpg --keyserver keyserver.ubuntu.com --search-keys your-email@example.com
   ```

   **Note:** The PPA workflow also handles this automatically. If the key
   is not yet on the keyserver at upload time, the workflow publishes it
   and schedules automatic retries every 20 minutes until the key
   propagates (see [GPG Keyserver Propagation](#gpg-keyserver-propagation)
   below).

### 1.2 Generate SSH Keys

You need two separate SSH keys: one for AUR and one for Launchpad. Using
separate keys limits the blast radius if one is compromised.

**Important:** For CI use, generate these keys **without a passphrase**. The
keys will be stored as GitHub secrets, which provides the security layer.

1. **Generate the AUR SSH key:**

   ```bash
   ssh-keygen -t ed25519 -C "aur-packaging" -f ~/.ssh/id_aur_ed25519 -N ""
   ```

   This creates:
   - `~/.ssh/id_aur_ed25519` — private key (goes into GitHub secret)
   - `~/.ssh/id_aur_ed25519.pub` — public key (registered on AUR)

2. **Generate the Launchpad SSH key:**

   ```bash
   ssh-keygen -t ed25519 -C "launchpad-packaging" -f ~/.ssh/id_launchpad_ed25519 -N ""
   ```

   This creates:
   - `~/.ssh/id_launchpad_ed25519` — private key (goes into GitHub secret)
   - `~/.ssh/id_launchpad_ed25519.pub` — public key (registered on Launchpad)

---

## Part 2: Set Up AUR Publishing

The [Arch User Repository (AUR)](https://aur.archlinux.org/) is a
community-driven repository for Arch Linux packages. Packages are published as
`PKGBUILD` files pushed via git over SSH.

### 2.1 Create an AUR Account

1. Go to https://aur.archlinux.org/register
2. Fill in:
   - **Username:** Choose a username.
   - **Email:** Use the packaging identity email.
   - **Password:** Choose a password.
3. Click **Create** to register.
4. Verify your email address by clicking the link sent to you.

### 2.2 Register Your SSH Key

1. Log in to the AUR at https://aur.archlinux.org/login
2. Click **My Account** in the top navigation.
3. Scroll down to the **SSH Public Key** field.
4. Paste the contents of your AUR public key:

   ```bash
   cat ~/.ssh/id_aur_ed25519.pub
   ```

   Copy the entire output (it starts with `ssh-ed25519` and ends with
   `aur-packaging`) and paste it into the field.
5. Click **Update** to save.

### 2.3 Register the Package on AUR

Before the workflow can push updates, the package must exist on the AUR.

1. **Clone the empty AUR package repo:**

   ```bash
   git clone ssh://aur@aur.archlinux.org/siomon.git /tmp/aur-siomon
   ```

   If the package doesn't exist yet, this creates an empty repo. If it already
   exists, this clones the current state.

2. **Copy the PKGBUILD and generate .SRCINFO:**

   If the repo is empty, copy the PKGBUILD from this repository and generate
   the initial `.SRCINFO`:

   ```bash
   cd /tmp/aur-siomon
   cp /path/to/siomon/packaging/aur/PKGBUILD .
   # You need makepkg installed (Arch Linux) to generate .SRCINFO:
   makepkg --printsrcinfo > .SRCINFO
   git add PKGBUILD .SRCINFO
   git commit -m "Initial commit"
   git push origin master
   ```

   After this initial push, the GitHub Actions workflow will handle all future
   updates automatically.

---

## Part 3: Set Up Launchpad PPA Publishing

[Launchpad](https://launchpad.net/) is Ubuntu's hosting platform for package
archives. A PPA (Personal Package Archive) lets you distribute `.deb` packages
for Ubuntu users. Launchpad builds binary packages from the signed source
packages that our workflow uploads.

### 3.1 Create a Launchpad Account

1. Go to https://login.launchpad.net/ and click **Create account** (you will
   need an Ubuntu One account).
2. Fill in your details and verify your email.
3. Once logged in, note your **Launchpad username** — you can find it in the
   URL when you visit your profile (e.g., `https://launchpad.net/~yourusername`).
   This username goes into the `LAUNCHPAD_LOGIN` secret.

### 3.2 Register Your GPG Key on Launchpad

Launchpad uses your GPG key to verify that uploaded source packages are
authentic.

1. Go to https://launchpad.net/~/+editpgpkeys (or navigate to your profile and
   click **Edit** next to "OpenPGP keys").
2. In the **Fingerprint** field, paste your GPG key fingerprint:

   ```bash
   gpg --fingerprint your-email@example.com
   ```

   Copy the 40-character fingerprint (spaces are OK, Launchpad strips them).
3. Click **Import Key**.
4. Launchpad will send an encrypted email to the address on the key. You need
   to decrypt it to confirm ownership:

   ```bash
   # The email contains an encrypted message. Save the encrypted text to a file,
   # then decrypt:
   gpg --decrypt launchpad-confirmation.txt
   ```

   The decrypted message contains a URL. Open it in your browser to confirm
   the key.

   **Note:** Make sure the GPG key's email matches an email verified on your
   Launchpad account, otherwise the confirmation email won't be sent.

### 3.3 Register Your SSH Key on Launchpad

Launchpad uses SSH keys for uploading packages via SFTP.

1. Go to https://launchpad.net/~/+editsshkeys (or navigate to your profile and
   click **Edit** next to "SSH keys").
2. Paste the contents of your Launchpad public key:

   ```bash
   cat ~/.ssh/id_launchpad_ed25519.pub
   ```

3. Click **Import Public Key**.

### 3.4 Create a PPA

1. Go to your Launchpad profile page (https://launchpad.net/~yourusername).
2. Click **Create a new PPA** in the left sidebar.
3. Fill in:
   - **URL:** A short name for the PPA (e.g., `siomon`). This becomes part
     of the PPA identifier: `ppa:yourusername/siomon`.
   - **Display name:** A human-readable name (e.g., `siomon packages`).
   - **Description:** A brief description.
4. Under **Processors**, make sure both **amd64** and **arm64** are checked
   (Launchpad will build binaries for these architectures).
5. Click **Activate** to create the PPA.

After creation, users can add your PPA and install packages with:

```bash
sudo add-apt-repository ppa:yourusername/siomon
sudo apt update
sudo apt install siomon
```

---

## Part 4: Configure GitHub Secrets

The workflows read all identity and credential information from GitHub
repository secrets. No credentials are hardcoded in the workflow files.

### 4.1 Required Secrets

| Secret | Required | Used By | Description |
|--------|----------|---------|-------------|
| `AUR_SSH_PRIVATE_KEY` | Yes | AUR | Private SSH key for pushing to AUR |
| `PKG_GPG_PRIVATE_KEY` | Yes | PPA | Armored GPG private key for signing packages |
| `PKG_GPG_PASSPHRASE` | No | PPA | GPG key passphrase (omit if key has no passphrase) |
| `PKG_GPG_KEY_ID` | Yes | PPA | GPG key email or ID used for signing |
| `PKG_GIT_NAME` | Yes | Both | Git author name for commits and changelogs |
| `PKG_GIT_EMAIL` | Yes | Both | Git author email for commits and changelogs |
| `LAUNCHPAD_SSH_PRIVATE_KEY` | Yes | PPA | Private SSH key for Launchpad SFTP upload |
| `LAUNCHPAD_LOGIN` | Yes | PPA | Your Launchpad username |

### 4.2 How to Add Secrets

1. Go to your repository on GitHub (e.g.,
   `https://github.com/level1techs/siomon`).
2. Click **Settings** in the top navigation.
3. In the left sidebar, expand **Secrets and variables** and click **Actions**.
4. Click **New repository secret**.
5. Enter the **Name** (exactly as shown in the table above, e.g.,
   `AUR_SSH_PRIVATE_KEY`) and paste the **Value**.
6. Click **Add secret**.
7. Repeat for each secret.

**Important:** Secret values are write-only. Once saved, you cannot view them
again (only overwrite). Make sure you have backups of your keys.

### 4.3 Obtaining Secret Values

#### `AUR_SSH_PRIVATE_KEY`

The entire contents of the private key file:

```bash
cat ~/.ssh/id_aur_ed25519
```

Copy everything, including the `-----BEGIN OPENSSH PRIVATE KEY-----` and
`-----END OPENSSH PRIVATE KEY-----` lines.

#### `LAUNCHPAD_SSH_PRIVATE_KEY`

Same as above but for the Launchpad key:

```bash
cat ~/.ssh/id_launchpad_ed25519
```

#### `PKG_GPG_PRIVATE_KEY`

Export the GPG private key in armored (text) format:

```bash
gpg --armor --export-secret-keys your-email@example.com
```

Copy the entire output, including the `-----BEGIN PGP PRIVATE KEY BLOCK-----`
and `-----END PGP PRIVATE KEY BLOCK-----` lines.

**Security note:** This exports your private key. Handle it carefully and
never share it outside of GitHub secrets.

#### `PKG_GPG_PASSPHRASE`

The passphrase you chose when generating the GPG key. If you generated the
key without a passphrase, you can skip this secret entirely — the workflow
handles both cases.

#### `PKG_GPG_KEY_ID`

The email address associated with your GPG key (e.g.,
`level1techspackageteam@gmail.com`). This is used by `debsign` to select the
correct key for signing.

You can verify it with:

```bash
gpg --list-secret-keys
```

The `uid` line shows the email.

#### `PKG_GIT_NAME`

The name used for git commits and Debian changelogs (e.g.,
`Level1Techs Package Team`).

#### `PKG_GIT_EMAIL`

The email used for git commits and Debian changelogs (e.g.,
`level1techspackageteam@gmail.com`). This should typically match your
`PKG_GPG_KEY_ID`.

#### `LAUNCHPAD_LOGIN`

Your Launchpad username. Find it by going to https://launchpad.net/ and
looking at your profile URL: `https://launchpad.net/~yourusername` — the
part after the `~` is your login.

---

## Part 5: Using the Workflows

### 5.1 Automatic Publishing (Tag Push)

When you push a version tag, everything happens automatically:

```bash
# Bump version in Cargo.toml, then:
git add Cargo.toml Cargo.lock
git commit -m "Bump version to 0.3.0"
git tag v0.3.0
git push origin main --tags
```

The Release workflow will:
1. Build binaries for x86_64 and aarch64.
2. Publish to crates.io.
3. Create a GitHub Release.
4. Trigger the AUR and PPA workflows in parallel.

### 5.2 Manual AUR Publishing

To manually trigger the AUR workflow (e.g., to re-publish a failed build):

1. Go to the repository's **Actions** tab on GitHub.
2. Select **Publish to AUR** in the left sidebar.
3. Click **Run workflow**.
4. Enter the **tag** (e.g., `v0.3.0`).
5. Click **Run workflow** to start.

The workflow automatically queries the AUR API to determine the current
`pkgrel`. If the upstream version (`pkgver`) already exists on the AUR, it
increments `pkgrel` (e.g., `1` → `2`). For a new upstream version, `pkgrel`
resets to `1`. This means re-running the workflow for the same tag always
produces a publishable update.

### 5.3 Manual PPA Publishing

To manually trigger the PPA workflow:

1. Go to the repository's **Actions** tab on GitHub.
2. Select **Publish to PPA** in the left sidebar.
3. Click **Run workflow**.
4. Fill in the inputs:
   - **tag** (required): The git tag (e.g., `v0.3.0`).
   - **series** (default: `noble questing`): Space-separated list of Ubuntu
     series to build for.
   - **ppa** (default: `ppa:level1techs/siomon`): The target PPA.
   - **upload** (default: checked): Uncheck to build and sign without
     uploading — useful for testing.
5. Click **Run workflow**.

The workflow automatically queries the Launchpad API to find the highest
published repack suffix (`+dsN`) and revision for the current upstream
version. For a new upstream version, repack starts at 1. For re-runs of
the same version, repack is incremented by 1 to produce a fresh orig
tarball name. Revision always resets to 1. This means re-running the
workflow for the same tag always produces a unique version that Launchpad
will accept.

The build artifacts (`.dsc`, `.changes`, `.orig.tar.gz`) are always uploaded
as GitHub Actions artifacts regardless of the upload setting, so you can
inspect them.

---

## Part 6: Troubleshooting

### AUR: "Host key verification failed"

The workflow uses `StrictHostKeyChecking=accept-new` to automatically accept
the AUR's SSH host key. If this fails:

- Verify the `AUR_SSH_PRIVATE_KEY` secret is set correctly (the entire
  private key file, including the BEGIN/END lines).
- Verify the corresponding public key is registered on your AUR account.
- Check if AUR is experiencing downtime at https://status.archlinux.org/.

### AUR: "Permission denied (publickey)"

The SSH key on your AUR account doesn't match the private key in the secret.

1. Regenerate the key pair and update both the AUR account and the GitHub
   secret.
2. Make sure you're copying the **private** key (not the `.pub` file) into the
   GitHub secret.

### GPG Keyserver Propagation

Launchpad requires the signing GPG key to be available on
`keyserver.ubuntu.com` before it will accept uploads. Key propagation can
take minutes to hours.

The PPA workflow handles this automatically:

1. Before uploading, the workflow checks if the GPG key is retrievable
   from `keyserver.ubuntu.com`.
2. If the key is already available, the upload proceeds immediately.
3. If not, the workflow publishes the key with `gpg --send-keys`, stores
   the pending upload parameters (including a `started_at` timestamp)
   in a `PPA_GPG_RETRY` repository variable, and dispatches the
   `gpg-keyserver-retry.yml` workflow.
4. The retry workflow uses the `gpg-retry-delay` GitHub environment
   (which must have a 20-minute wait timer configured). The runner is
   not allocated during the wait, so there is no runner cost. Once the
   timer expires, it checks the keyserver. If the key is available, it
   cleans up the variable and dispatches the full PPA workflow. If not,
   it dispatches itself for the next check.
5. A 6-hour timeout prevents infinite retries.

The retry workflow only runs when explicitly dispatched — there is no
cron schedule. It produces zero workflow runs when no retry is pending.

**Required one-time setup**: Create a `gpg-retry-delay` environment in
the repository (Settings → Environments → New environment) with a
20-minute wait timer protection rule. No required reviewers.

You can also monitor and manage this manually:

```bash
# Check if a retry is pending
gh variable get PPA_GPG_RETRY

# Cancel a pending retry
gh variable delete PPA_GPG_RETRY
```

### PPA: "Signature verification failed"

Launchpad rejected the signature. Common causes:

- The GPG key in `PKG_GPG_PRIVATE_KEY` doesn't match the fingerprint
  registered on your Launchpad account.
- The `PKG_GPG_KEY_ID` doesn't match the key's email.
- The key hasn't been uploaded to `keyserver.ubuntu.com` yet, or hasn't
  propagated. The workflow handles this automatically (see
  [GPG Keyserver Propagation](#gpg-keyserver-propagation) above), but
  you can also re-upload manually:

  ```bash
  gpg --keyserver keyserver.ubuntu.com --send-keys YOUR_FINGERPRINT
  ```

### PPA: "Already uploaded"

Launchpad rejects re-uploads of the same version. The workflow automatically
queries the Launchpad API and increments both the repack suffix and revision,
so this should not normally occur. If it does:

- The API query may have failed or returned stale data. Re-run the workflow.

### PPA: Build failures on Launchpad

After uploading, Launchpad builds binary packages in clean chroots. If the
build fails:

1. Check the build log on your PPA page (e.g.,
   `https://launchpad.net/~yourusername/+archive/ubuntu/siomon/+packages`).
2. Common issues:
   - Missing build dependencies in `packaging/launchpad/debian/control`.
   - Rust version too old in the target series (the workflow handles noble
     specially with `cargo-1.85`/`rustc-1.85`).
   - Vendored dependencies are incomplete — re-run the workflow (repack
     increments automatically).

### Workflow: Secrets not available

If the workflow fails with empty secret values:

- Secrets are repository-scoped. Make sure they are added to the correct
  repository under **Settings > Secrets and variables > Actions**.
- For forked repositories, secrets from the parent are not inherited. You
  need to add them to the fork separately.
- Secret names are case-sensitive. Verify they match exactly (e.g.,
  `AUR_SSH_PRIVATE_KEY`, not `aur_ssh_private_key`).
