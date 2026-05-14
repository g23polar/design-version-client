# DSV User Guide
## A Simple Backup Tool for Design Files

**DSV** (Design Snapshot Version) is a command-line tool that helps you keep track of different versions of your design files. Think of it like a specialized backup system that's particularly good at handling large files like Photoshop documents, 3D models, and PDFs.

---

## What DSV Does

DSV is like a **smart backup system** for your design work:

- 📁 **Takes snapshots** of your files so you can go back to older versions
- 💾 **Saves space** by not duplicating identical files
- 🏷️ **Labels versions** so you can remember what each snapshot was for
- 🔍 **Shows differences** between versions
- 📂 **Works with whole folders** of files at once
- 🧹 **Cleans up** old files you don't need anymore

---

## Getting Started

### 1. Open Your Terminal

**On Mac:** Press `Cmd + Space`, type "Terminal", press Enter  
**On Windows:** Press `Windows + R`, type "cmd", press Enter  
**On Linux:** Press `Ctrl + Alt + T`

### 2. Navigate to Your Project Folder

Use the `cd` command to go to the folder with your design files:

```bash
cd /path/to/your/design/project
```

**Examples:**
```bash
cd ~/Documents/MyProject          # Mac/Linux
cd C:\Users\YourName\MyProject    # Windows
```

### 3. Initialize DSV in Your Project

This creates a hidden `.dsv` folder to store your snapshots:

```bash
dsv init
```

You should see: `✓ Initialized design version store`

---

## Basic Commands

### Taking Your First Snapshot

**For a single file:**
```bash
dsv snapshot my-design.psd
```

**For an entire folder:**
```bash
dsv snapshot-dir .
```
*(The `.` means "current folder")*

**With a descriptive label:**
```bash
dsv snapshot my-design.psd --label "Initial logo design"
```

### Viewing Your Snapshots

**See all snapshots:**
```bash
dsv list
```

**See snapshots for a specific file:**
```bash
dsv list my-design.psd
```

**See snapshots with a certain label:**
```bash
dsv list --label "final"
```

### Getting Files Back

**Restore to a new location:**
```bash
dsv restore --id abc123 --out restored-design.psd
```

**Restore a whole batch of files:**
```bash
dsv restore-batch --batch def456 --out-dir recovered-files/
```

---

## Real-World Workflows

### Scenario 1: Working on a Logo Design

```bash
# Start your project
cd ~/Documents/LogoProject
dsv init

# Save your initial concept
dsv snapshot logo-v1.psd --label "initial concept"

# Work on it, then save another version
dsv snapshot logo-v2.psd --label "added color variations"

# Save the final version
dsv snapshot logo-final.psd --label "client approved final"

# See your progress
dsv list
```

### Scenario 2: Daily Work Backup

```bash
# At the end of each day, backup everything
dsv snapshot-dir . --label "end of day backup"

# See what you've been working on
dsv list --today
```

### Scenario 3: Before Making Big Changes

```bash
# Before trying something risky
dsv snapshot my-complex-design.psd --label "before major revision"

# If things go wrong, you can get it back
dsv list my-complex-design.psd
dsv restore --id abc123 --out my-complex-design-recovered.psd
```

---

## Understanding the Output

When you run `dsv list`, you'll see something like:

```
ID       | File              | Size    | Label           | Date
---------|-------------------|---------|-----------------|------------------
a1b2c3   | logo.psd         | 45 MB   | initial concept | 2024-01-15 09:30
d4e5f6   | logo.psd         | 47 MB   | added colors    | 2024-01-15 14:20
g7h8i9   | logo.psd         | 46 MB   | final version   | 2024-01-16 11:45
```

- **ID:** A unique code for this snapshot (use this to restore)
- **File:** The original filename
- **Size:** How big the file is
- **Label:** Your description (if you added one)
- **Date:** When you took the snapshot

---

## Useful Tips

### Adding Labels Later

If you forgot to add a label when taking a snapshot:
```bash
dsv label --id abc123 "final version for client"
```

### Comparing Versions

See what changed between two snapshots:
```bash
dsv diff --from abc123 --to def456
```

### Cleaning Up Old Files

See how much space you could free up:
```bash
dsv gc
```

Actually delete old unused files (be careful!):
```bash
dsv gc --confirm
```

### Deleting Specific Snapshots

Delete a single snapshot:
```bash
dsv delete --id abc123 --confirm
```

Delete all files from a folder snapshot:
```bash
dsv delete --batch def456 --confirm
```

---

## File Types That Work Well

DSV is designed for large binary files, especially:

- **Design Files:** `.psd`, `.psb`, `.ai`, `.sketch`, `.fig`
- **3D Models:** `.3dm`, `.max`, `.blend`, `.obj`, `.fbx`
- **Documents:** `.pdf`, `.indd`, `.docx`
- **Images:** `.png`, `.jpg`, `.tiff`, `.raw`
- **Video:** `.mp4`, `.mov`, `.avi`
- **Any large file** you want to version

---

## Common Questions

### "How is this different from just copying files?"

DSV is smarter about storage - if you snapshot the same file twice, it only stores one copy. It also keeps organized records of what you saved and when.

### "What if I accidentally delete something?"

As long as you took a snapshot, you can get it back! Use `dsv list` to find it, then `dsv restore` to get it back.

### "Can I use this with my team?"

DSV stores everything locally on your computer. For team sharing, you'd need to set up a shared folder or use additional tools.

### "Is it safe?"

Yes! DSV never modifies your original files. It makes copies and stores them safely. The `--confirm` flags prevent accidental deletions.

### "What if I mess up a command?"

Most commands are safe by default. Commands that delete things require `--confirm` and show you what they'll do first. You can always run commands without `--confirm` to see what would happen.

---

## Quick Reference

| What you want to do | Command |
|---------------------|---------|
| Set up DSV in a new project | `dsv init` |
| Save one file | `dsv snapshot filename.psd` |
| Save a whole folder | `dsv snapshot-dir .` |
| Save with a note | `dsv snapshot file.psd --label "description"` |
| See all your snapshots | `dsv list` |
| Get a file back | `dsv restore --id ABC123 --out newname.psd` |
| See what changed | `dsv diff --from ABC123 --to DEF456` |
| Add a note to old snapshot | `dsv label --id ABC123 "final version"` |
| Clean up old files | `dsv gc --confirm` |
| Delete a snapshot | `dsv delete --id ABC123 --confirm` |
| Check if files are OK | `dsv verify` |

---

## Getting Help

**In the terminal:**
```bash
dsv --help                    # General help
dsv snapshot --help           # Help for specific command
```

**Common error messages:**

- `No .dsv directory found` → Run `dsv init` first
- `File not found` → Check your file path and spelling
- `Permission denied` → You might need admin rights for that folder

---

## Safety Notes

1. **Always backup important work** in multiple places (DSV is one layer of protection)
2. **Test restore commands** on unimportant files first to get comfortable
3. **Use meaningful labels** so you can find things later
4. **Run `gc` without `--confirm` first** to see what would be deleted
5. **The `--confirm` flag means "really do it"** - commands without it just show what would happen

---

*DSV is designed to be your safety net for creative work. When in doubt, take a snapshot - storage is cheap, but recreating lost work isn't!*