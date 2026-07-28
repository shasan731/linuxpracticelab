#!/usr/bin/env tsx
/**
 * Deterministically authors the five missing MVP curriculum modules from the product
 * specification. The generated JSON remains the runtime source of truth; this script keeps the
 * repeated lesson structure consistent and makes future editorial changes reviewable.
 */

import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const scriptDir = fileURLToPath(new URL('.', import.meta.url));
const repoRoot = resolve(scriptDir, '..');
const coreRoot = join(repoRoot, 'lessons', 'core');
const setupRoot = join(repoRoot, 'lessons', 'assets', 'setup');

type Validator = { type: string; [key: string]: unknown };

interface TaskSeed {
  instruction: string;
  solution: string;
  validators: Validator[];
  alternate?: string[];
  incorrect: string;
  hints?: string[];
  kind?: 'guided' | 'independent' | 'applied' | 'assessment';
  context?: string;
}

interface LessonSeed {
  id: string;
  title: string;
  purpose: string;
  mentalModel: string;
  commands: string[];
  syntax: string[];
  concepts: string[];
  tasks: TaskSeed[];
  setup?: string;
  assessment?: boolean;
  minutes?: number;
  difficulty?: number;
}

interface ModuleSeed {
  id: string;
  number: number;
  title: string;
  level: 'beginner' | 'foundation';
  summary: string;
  outcomes: string[];
  lessons: LessonSeed[];
}

const sha256 = (content: string) => createHash('sha256').update(content).digest('hex');
const file = (path: string): Validator => ({ type: 'file_exists', path });
const missing = (path: string): Validator => ({ type: 'file_missing', path });
const directory = (path: string): Validator => ({ type: 'directory_exists', path });
const directoryMissing = (path: string): Validator => ({ type: 'directory_missing', path });
const contains = (path: string, text: string, extra: Record<string, unknown> = {}): Validator => ({
  type: 'file_contains',
  path,
  text,
  ...extra,
});
const exact = (path: string, content: string): Validator[] => [
  file(path),
  { type: 'file_checksum', path, sha256: sha256(content) },
];
const lineCount = (path: string, equals: number): Validator => ({
  type: 'file_line_count',
  path,
  equals,
});
const entries = (
  path: string,
  names: string[],
  exactEntries = true,
  includeHidden = true,
): Validator => ({
  type: 'directory_contains',
  path,
  entries: names,
  exact: exactEntries,
  includeHidden,
});

function labSetup(...body: string[]): string {
  return [
    '#!/usr/bin/env bash',
    'set -euo pipefail',
    'student_home=/home/student',
    'rm -rf -- "${student_home}/lab" "${student_home}/answers"',
    'mkdir -p "${student_home}/lab" "${student_home}/answers"',
    ...body,
    'chown -R student:student "${student_home}/lab" "${student_home}/answers"',
    '',
  ].join('\n');
}

const modules: ModuleSeed[] = [
  {
    id: 'filesystem-navigation',
    number: 2,
    title: 'Filesystem navigation',
    level: 'beginner',
    summary:
      'Read the Linux directory tree, inspect locations, and move confidently with absolute, relative, hidden, parent, and home-directory paths.',
    outcomes: [
      'Explain the purpose of the major top-level Linux directories',
      'Use pwd and ls to establish location and available paths',
      'Navigate with absolute paths, relative paths, dot, dot-dot, tilde and cd dash',
      'Find hidden entries and complete an unfamiliar navigation challenge',
    ],
    lessons: [
      {
        id: 'filesystem.01',
        title: 'Linux Directory Tree',
        purpose: 'Recognise where Linux keeps configuration, user data, logs, devices and programs.',
        mentalModel:
          'Linux presents one tree rooted at /. Top-level directories are conventions with distinct jobs, not separate drive letters.',
        commands: ['ls'],
        syntax: ['ls /', 'ls -d /etc /home /var'],
        concepts: ['root directory', 'filesystem hierarchy', 'top-level directories'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Inspect the root directory, then save the paths /etc, /home and /var as three lines in /home/student/answers/directory-tree.txt.',
            solution: "ls -d /etc /home /var > /home/student/answers/directory-tree.txt",
            alternate: [
              "printf '/etc\\n/home\\n/var\\n' > /home/student/answers/directory-tree.txt",
            ],
            incorrect: "printf '/tmp\\n' > /home/student/answers/directory-tree.txt",
            validators: exact(
              '/home/student/answers/directory-tree.txt',
              '/etc\n/home\n/var\n',
            ),
            hints: [
              'The tree begins at /, and ls can show selected paths as well as directory contents.',
              'Use ls with the -d option so the directory names themselves are printed.',
              'Redirect the three required absolute paths into the answer file.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.02',
        title: 'Current Directory with pwd',
        purpose: 'Confirm exactly where a shell operation will read or write files.',
        mentalModel:
          'Every shell process has a current working directory. Relative paths start there, and pwd asks the shell to print its absolute location.',
        commands: ['pwd', 'cd'],
        syntax: ['pwd', 'cd /var/log && pwd'],
        concepts: ['working directory', 'absolute location'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Move to /var/log and record the directory reported by pwd in /home/student/answers/location.txt.',
            solution: 'cd /var/log && pwd > /home/student/answers/location.txt',
            alternate: ['cd /var/log; pwd > ~/answers/location.txt'],
            incorrect: 'cd /var && pwd > /home/student/answers/location.txt',
            validators: exact('/home/student/answers/location.txt', '/var/log\n'),
            hints: [
              'Change location first, then ask the shell where it is.',
              'The two commands are cd and pwd.',
              'Join the commands with && and redirect pwd into the requested file.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.03',
        title: 'Listing Files with ls',
        purpose: 'Inspect a directory before choosing a file or changing anything.',
        mentalModel:
          'ls reads directory entries. Options change which entries are shown and how their metadata is presented; they do not change the files.',
        commands: ['ls', 'sort'],
        syntax: ['ls', 'ls -la', 'ls -Ah DIRECTORY'],
        concepts: ['directory entries', 'long listing', 'hidden entries'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/catalog"',
          'printf "quarterly figures\\n" > "${student_home}/lab/catalog/report.txt"',
          'printf "remember hidden files\\n" > "${student_home}/lab/catalog/.hidden-note"',
        ),
        tasks: [
          {
            instruction:
              'List every entry in /home/student/lab/catalog except . and .., sort the names, and save them to /home/student/answers/catalog-list.txt.',
            solution:
              'ls -A /home/student/lab/catalog | sort > /home/student/answers/catalog-list.txt',
            alternate: [
              "find /home/student/lab/catalog -mindepth 1 -maxdepth 1 -printf '%f\\n' | sort > /home/student/answers/catalog-list.txt",
            ],
            incorrect:
              'ls /home/student/lab/catalog > /home/student/answers/catalog-list.txt',
            validators: exact(
              '/home/student/answers/catalog-list.txt',
              '.hidden-note\nreport.txt\n',
            ),
            hints: [
              'The ordinary listing hides names beginning with a dot.',
              'Use ls -A to include hidden entries without adding . and ...',
              'Pipe the listing through sort before redirecting it.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.04',
        title: 'Changing Directories with cd',
        purpose: 'Make another directory the starting point for relative paths.',
        mentalModel:
          'cd changes the working directory of the current shell, which is why it must be a shell builtin rather than a separate program.',
        commands: ['cd', 'pwd'],
        syntax: ['cd DIRECTORY', 'cd -', 'cd ~'],
        concepts: ['shell builtin', 'working directory', 'previous directory'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/start" "${student_home}/lab/target"',
        ),
        tasks: [
          {
            instruction:
              'Start in /home/student/lab/start, move to the sibling target directory, and save the resulting pwd output to /home/student/answers/cd-result.txt.',
            solution:
              'cd /home/student/lab/start && cd ../target && pwd > /home/student/answers/cd-result.txt',
            alternate: [
              'cd /home/student/lab/target && pwd > /home/student/answers/cd-result.txt',
            ],
            incorrect:
              'cd /home/student/lab/start && pwd > /home/student/answers/cd-result.txt',
            validators: exact(
              '/home/student/answers/cd-result.txt',
              '/home/student/lab/target\n',
            ),
            hints: [
              'The target is beside start, so first move to the parent.',
              'Use cd .. and then cd target, or name the target directly.',
              'Run pwd only after the final cd and redirect that output.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.05',
        title: 'Absolute Paths',
        purpose: 'Reach the same location regardless of the shell’s starting directory.',
        mentalModel:
          'An absolute path begins with / and is resolved from the root of the filesystem, so the current directory cannot change its meaning.',
        commands: ['cd', 'pwd'],
        syntax: ['cd /usr/share/doc', 'pwd'],
        concepts: ['absolute paths', 'root directory'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'From any starting location, use an absolute path to reach /usr/share/doc and record pwd in /home/student/answers/absolute-path.txt.',
            solution: 'cd /usr/share/doc && pwd > /home/student/answers/absolute-path.txt',
            alternate: [
              'cd /tmp; cd /usr/share/doc; pwd > /home/student/answers/absolute-path.txt',
            ],
            incorrect: 'cd /usr/share && pwd > /home/student/answers/absolute-path.txt',
            validators: exact(
              '/home/student/answers/absolute-path.txt',
              '/usr/share/doc\n',
            ),
            hints: [
              'Absolute paths always begin at the root directory.',
              'Use cd with the full path beginning with /.',
              'Record pwd after reaching the documentation directory.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.06',
        title: 'Relative Paths',
        purpose: 'Navigate nearby directories without repeatedly typing a complete path.',
        mentalModel:
          'A relative path has no leading slash. The shell joins it to the current working directory before looking it up.',
        commands: ['cd', 'pwd'],
        syntax: ['cd reports', 'cd projects/reports'],
        concepts: ['relative paths', 'working directory', 'child directory'],
        setup: labSetup('mkdir -p "${student_home}/lab/projects/reports"'),
        tasks: [
          {
            instruction:
              'Move into /home/student/lab/projects, enter reports using only its relative name, and save pwd to /home/student/answers/relative-path.txt.',
            solution:
              'cd /home/student/lab/projects && cd reports && pwd > /home/student/answers/relative-path.txt',
            alternate: [
              'cd /home/student/lab && cd projects/reports && pwd > /home/student/answers/relative-path.txt',
            ],
            incorrect:
              'cd /home/student/lab/projects && cd /reports && pwd > /home/student/answers/relative-path.txt',
            validators: exact(
              '/home/student/answers/relative-path.txt',
              '/home/student/lab/projects/reports\n',
            ),
            hints: [
              'A child directory can be named relative to the current directory.',
              'Once inside projects, the relative path is simply reports.',
              'Do not put / before reports; that would start at the filesystem root.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.07',
        title: 'Dot and Dot-Dot',
        purpose: 'Refer explicitly to the current directory and its parent.',
        mentalModel:
          '. is the current directory entry and .. is the parent entry. They are real path components that can appear anywhere in a relative path.',
        commands: ['cd', 'pwd'],
        syntax: ['cd .', 'cd ..', 'cd ../sibling'],
        concepts: ['current directory', 'parent directory', 'path components'],
        setup: labSetup('mkdir -p "${student_home}/lab/tree/parent/child"'),
        tasks: [
          {
            instruction:
              'Enter /home/student/lab/tree/parent/child, return to its parent with dot-dot, and save pwd to /home/student/answers/parent-path.txt.',
            solution:
              'cd /home/student/lab/tree/parent/child && cd .. && pwd > /home/student/answers/parent-path.txt',
            alternate: [
              'cd /home/student/lab/tree/parent/child/.. && pwd > /home/student/answers/parent-path.txt',
            ],
            incorrect:
              'cd /home/student/lab/tree/parent/child && cd . && pwd > /home/student/answers/parent-path.txt',
            validators: exact(
              '/home/student/answers/parent-path.txt',
              '/home/student/lab/tree/parent\n',
            ),
            hints: [
              'One dot stays in place; two dots move one level upward.',
              'Use cd .. after entering child.',
              'Run pwd from the parent directory and redirect it.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.08',
        title: 'Hidden Files',
        purpose: 'Find configuration and state files whose names begin with a dot.',
        mentalModel:
          'A leading dot is only a naming convention. Ordinary listings omit such entries, but the files have normal paths and permissions.',
        commands: ['ls', 'find', 'sort'],
        syntax: ['ls -a', 'ls -la', "find DIRECTORY -name '.*'"],
        concepts: ['dotfiles', 'hidden entries'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/profile"',
          'printf "visible\\n" > "${student_home}/lab/profile/note.txt"',
          'printf "hidden\\n" > "${student_home}/lab/profile/.hidden-note"',
        ),
        tasks: [
          {
            instruction:
              'Find the hidden entry in /home/student/lab/profile and save only its name to /home/student/answers/hidden-files.txt.',
            solution:
              "find /home/student/lab/profile -mindepth 1 -maxdepth 1 -name '.*' -printf '%f\\n' | sort > /home/student/answers/hidden-files.txt",
            alternate: [
              "ls -A /home/student/lab/profile | grep '^\\.' > /home/student/answers/hidden-files.txt",
            ],
            incorrect:
              'ls /home/student/lab/profile > /home/student/answers/hidden-files.txt',
            validators: exact('/home/student/answers/hidden-files.txt', '.hidden-note\n'),
            hints: [
              'Hidden names start with a dot and ordinary ls omits them.',
              'Use ls -A, or find with a name pattern beginning with a dot.',
              'Save the entry name, not its contents.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.09',
        title: 'Home Directory Shortcuts',
        purpose: 'Return to the current user’s home directory without hard-coding its path.',
        mentalModel:
          '~ is expanded by the shell and $HOME is an environment variable. With no argument, cd also chooses the current user’s home.',
        commands: ['cd', 'pwd', 'echo'],
        syntax: ['cd', 'cd ~', 'echo "$HOME"'],
        concepts: ['home directory', 'tilde expansion', 'environment variables'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Move to /tmp, return home using a home-directory shortcut, and save pwd to /home/student/answers/home-shortcut.txt.',
            solution:
              'cd /tmp && cd ~ && pwd > /home/student/answers/home-shortcut.txt',
            alternate: [
              'cd /tmp && cd "$HOME" && pwd > /home/student/answers/home-shortcut.txt',
              'cd /tmp && cd && pwd > /home/student/answers/home-shortcut.txt',
            ],
            incorrect:
              'cd /tmp && pwd > /home/student/answers/home-shortcut.txt',
            validators: exact(
              '/home/student/answers/home-shortcut.txt',
              '/home/student\n',
            ),
            hints: [
              'The shell has more than one way to name your home directory.',
              'Use cd ~, cd "$HOME", or cd with no argument.',
              'Record pwd only after returning home.',
            ],
          },
        ],
      },
      {
        id: 'filesystem.10',
        title: 'Navigation Challenge',
        purpose: 'Combine path-reading skills inside an unfamiliar directory tree.',
        mentalModel:
          'Reliable navigation is a loop: establish location, inspect entries, move one deliberate step, and verify again.',
        commands: ['cd', 'pwd', 'find'],
        syntax: ['pwd', 'find PATH -name NAME', 'cd ../sibling'],
        concepts: ['navigation strategy', 'hidden files', 'relative paths'],
        assessment: true,
        difficulty: 3,
        minutes: 15,
        setup: labSetup(
          'mkdir -p "${student_home}/lab/maze/start" "${student_home}/lab/maze/project/reports"',
          'printf "navigation complete\\n" > "${student_home}/lab/maze/project/.clue"',
        ),
        tasks: [
          {
            kind: 'assessment',
            instruction:
              'Starting at /home/student/lab/maze/start, navigate into the project reports directory with a relative path. Create /home/student/answers/navigation-challenge.txt containing the final pwd on line one and the absolute path of the hidden .clue file on line two.',
            solution:
              "cd /home/student/lab/maze/start && cd ../project/reports && { pwd; find /home/student/lab/maze/project -name .clue -print; } > /home/student/answers/navigation-challenge.txt",
            alternate: [
              "cd /home/student/lab/maze/start && cd ../project/reports && printf '%s\\n%s\\n' \"$PWD\" \"$(readlink -f ../.clue)\" > /home/student/answers/navigation-challenge.txt",
            ],
            incorrect:
              "printf '/home/student/lab/maze/start\\n.clue\\n' > /home/student/answers/navigation-challenge.txt",
            validators: exact(
              '/home/student/answers/navigation-challenge.txt',
              '/home/student/lab/maze/project/reports\n/home/student/lab/maze/project/.clue\n',
            ),
          },
        ],
      },
    ],
  },
  {
    id: 'file-management',
    number: 3,
    title: 'Creating and managing files',
    level: 'beginner',
    summary:
      'Create, copy, move, remove, inspect and link files safely, including directories, wildcard groups and names containing spaces.',
    outcomes: [
      'Create directory trees and empty files',
      'Copy, move and remove files and directories without collateral damage',
      'Use wildcards and quoting deliberately',
      'Inspect metadata and distinguish hard links from symbolic links',
      'Organise a disordered project directory in a state-based assessment',
    ],
    lessons: [
      {
        id: 'files.01',
        title: 'Creating Directories with mkdir',
        purpose: 'Create places for related files and build nested directory structures.',
        mentalModel:
          'A directory is a mapping from names to filesystem objects. mkdir creates one mapping level; -p also creates any missing parents.',
        commands: ['mkdir', 'ls'],
        syntax: ['mkdir reports', 'mkdir -p projects/api/logs'],
        concepts: ['directories', 'parent directories'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Create /home/student/lab/company/reports/daily, /home/student/lab/company/reports/monthly and /home/student/lab/company/backups.',
            solution:
              'mkdir -p /home/student/lab/company/reports/{daily,monthly} /home/student/lab/company/backups',
            alternate: [
              'install -d /home/student/lab/company/reports/daily /home/student/lab/company/reports/monthly /home/student/lab/company/backups',
            ],
            incorrect: 'mkdir /home/student/lab/company',
            validators: [
              directory('/home/student/lab/company/reports/daily'),
              directory('/home/student/lab/company/reports/monthly'),
              directory('/home/student/lab/company/backups'),
            ],
            hints: [
              'Several parent directories do not exist yet.',
              'mkdir -p creates missing parents as part of the same operation.',
              'Brace expansion can name daily and monthly together, but separate commands also pass.',
            ],
          },
        ],
      },
      {
        id: 'files.02',
        title: 'Creating Empty Files with touch',
        purpose: 'Create placeholder files and update timestamps without opening an editor.',
        mentalModel:
          'touch updates access and modification times. When a named file does not exist, its default effect is to create an empty regular file.',
        commands: ['touch'],
        syntax: ['touch notes.txt', 'touch report-{1,2,3}.txt'],
        concepts: ['empty files', 'timestamps', 'brace expansion'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Create the three empty files report-1.txt, report-2.txt and report-3.txt in /home/student/lab.',
            solution: 'touch /home/student/lab/report-{1,2,3}.txt',
            alternate: [
              'cd /home/student/lab && touch report-1.txt report-2.txt report-3.txt',
            ],
            incorrect: 'touch /home/student/lab/report.txt',
            validators: [
              file('/home/student/lab/report-1.txt'),
              file('/home/student/lab/report-2.txt'),
              file('/home/student/lab/report-3.txt'),
              { type: 'file_size', path: '/home/student/lab/report-1.txt', equals: 0 },
            ],
            hints: [
              'The required files do not need contents yet.',
              'touch creates a missing file.',
              'Brace expansion can generate the three numbered names.',
            ],
          },
        ],
      },
      {
        id: 'files.03',
        title: 'Copying Files with cp',
        purpose: 'Duplicate a file while keeping the original in place.',
        mentalModel:
          'cp reads bytes from a source and writes a separate destination object. Later changes to one copy do not affect the other.',
        commands: ['cp'],
        syntax: ['cp SOURCE DESTINATION', 'cp FILE DIRECTORY/'],
        concepts: ['copying', 'source and destination'],
        setup: labSetup('printf "database=reports\\n" > "${student_home}/lab/source.conf"'),
        tasks: [
          {
            instruction:
              'Copy /home/student/lab/source.conf to /home/student/lab/backup.conf without changing or removing the source.',
            solution:
              'cp /home/student/lab/source.conf /home/student/lab/backup.conf',
            alternate: [
              'install -m 0644 /home/student/lab/source.conf /home/student/lab/backup.conf',
            ],
            incorrect: 'mv /home/student/lab/source.conf /home/student/lab/backup.conf',
            validators: [
              ...exact('/home/student/lab/source.conf', 'database=reports\n'),
              ...exact('/home/student/lab/backup.conf', 'database=reports\n'),
            ],
            hints: [
              'The original must still exist afterwards.',
              'cp takes a source followed by a destination.',
              'Name backup.conf as the second path.',
            ],
          },
        ],
      },
      {
        id: 'files.04',
        title: 'Copying Directories',
        purpose: 'Duplicate a directory tree, including every file beneath it.',
        mentalModel:
          'A directory copy walks the tree recursively. -r performs the walk; -a also preserves useful metadata such as modes and timestamps.',
        commands: ['cp'],
        syntax: ['cp -r DIRECTORY COPY', 'cp -a DIRECTORY ARCHIVE/'],
        concepts: ['recursive copies', 'metadata preservation'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/website/assets"',
          'printf "<h1>Linux Lab</h1>\\n" > "${student_home}/lab/website/index.html"',
          'printf "theme=dark\\n" > "${student_home}/lab/website/assets/site.conf"',
        ),
        tasks: [
          {
            instruction:
              'Create a complete copy of /home/student/lab/website at /home/student/lab/website-backup.',
            solution:
              'cp -a /home/student/lab/website /home/student/lab/website-backup',
            alternate: [
              'cp -r /home/student/lab/website /home/student/lab/website-backup',
            ],
            incorrect:
              'cp /home/student/lab/website/index.html /home/student/lab/website-backup',
            validators: [
              directory('/home/student/lab/website-backup/assets'),
              ...exact(
                '/home/student/lab/website-backup/index.html',
                '<h1>Linux Lab</h1>\n',
              ),
              ...exact(
                '/home/student/lab/website-backup/assets/site.conf',
                'theme=dark\n',
              ),
            ],
            hints: [
              'The source is a directory, so a plain file copy is not enough.',
              'Use cp recursively with -r, or preserve the tree with -a.',
              'The destination should be website-backup, not a single file inside it.',
            ],
          },
        ],
      },
      {
        id: 'files.05',
        title: 'Moving and Renaming with mv',
        purpose: 'Change a path without duplicating the underlying file.',
        mentalModel:
          'Within one filesystem, mv usually changes directory entries rather than copying file data. A new name and a new parent directory are the same operation.',
        commands: ['mv'],
        syntax: ['mv OLD NEW', 'mv FILE DIRECTORY/'],
        concepts: ['renaming', 'moving files'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/documents"',
          'printf "final report\\n" > "${student_home}/lab/draft.txt"',
        ),
        tasks: [
          {
            instruction:
              'Rename draft.txt to quarterly-report.txt and place it inside /home/student/lab/documents.',
            solution:
              'mv /home/student/lab/draft.txt /home/student/lab/documents/quarterly-report.txt',
            alternate: [
              'cd /home/student/lab && mv draft.txt documents/quarterly-report.txt',
            ],
            incorrect:
              'cp /home/student/lab/draft.txt /home/student/lab/documents/quarterly-report.txt',
            validators: [
              missing('/home/student/lab/draft.txt'),
              ...exact(
                '/home/student/lab/documents/quarterly-report.txt',
                'final report\n',
              ),
            ],
            hints: [
              'The original path must disappear, so this is not a copy.',
              'mv accepts the old path followed by the complete new path.',
              'The destination path can rename and move in one command.',
            ],
          },
        ],
      },
      {
        id: 'files.06',
        title: 'Removing Files with rm',
        purpose: 'Delete selected files deliberately and verify the pattern first.',
        mentalModel:
          'rm unlinks names immediately; it does not use a recycle bin. Permissions and open file handles may delay reclaimed storage, but the path is gone.',
        commands: ['rm', 'ls'],
        syntax: ['rm FILE', 'rm -i FILE'],
        concepts: ['deletion', 'interactive confirmation', 'no recycle bin'],
        setup: labSetup(
          'printf "discard\\n" > "${student_home}/lab/remove-me.txt"',
          'printf "keep\\n" > "${student_home}/lab/keep-me.txt"',
        ),
        tasks: [
          {
            instruction:
              'Remove only /home/student/lab/remove-me.txt. The neighbouring keep-me.txt must survive unchanged.',
            solution: 'rm /home/student/lab/remove-me.txt',
            alternate: ['unlink /home/student/lab/remove-me.txt'],
            incorrect: 'rm /home/student/lab/keep-me.txt',
            validators: [
              missing('/home/student/lab/remove-me.txt'),
              ...exact('/home/student/lab/keep-me.txt', 'keep\n'),
            ],
            hints: [
              'Name the exact file rather than using a broad wildcard.',
              'rm removes a file path; unlink is an equivalent single-file operation.',
              'Check that the path ends in remove-me.txt before pressing Enter.',
            ],
          },
        ],
      },
      {
        id: 'files.07',
        title: 'Removing Directories',
        purpose: 'Remove empty directories and deliberately remove populated trees when required.',
        mentalModel:
          'rmdir only succeeds for empty directories. Recursive rm walks through a populated tree, which is powerful because every descendant is affected.',
        commands: ['rmdir', 'rm'],
        syntax: ['rmdir EMPTY_DIRECTORY', 'rm -r POPULATED_DIRECTORY'],
        concepts: ['empty directories', 'recursive removal'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/empty" "${student_home}/lab/old-cache/nested"',
          'printf "cache\\n" > "${student_home}/lab/old-cache/nested/item.tmp"',
          'mkdir -p "${student_home}/lab/keep"',
        ),
        tasks: [
          {
            instruction:
              'Remove the empty directory and the populated old-cache tree from /home/student/lab, but leave keep in place.',
            solution:
              'rmdir /home/student/lab/empty && rm -r /home/student/lab/old-cache',
            alternate: [
              'rm -d /home/student/lab/empty && rm -rf /home/student/lab/old-cache',
            ],
            incorrect: 'rm -r /home/student/lab/keep',
            validators: [
              directoryMissing('/home/student/lab/empty'),
              directoryMissing('/home/student/lab/old-cache'),
              directory('/home/student/lab/keep'),
            ],
            hints: [
              'The two directories need different levels of removal.',
              'Use rmdir for the empty one and rm -r for the populated tree.',
              'Name both targets explicitly so keep is never part of the command.',
            ],
          },
        ],
      },
      {
        id: 'files.08',
        title: 'Wildcards',
        purpose: 'Select a predictable group of paths without listing every name.',
        mentalModel:
          'The shell expands wildcard patterns before starting the command. The receiving command gets a concrete list of paths, so previewing with ls shows the same expansion.',
        commands: ['ls', 'cp'],
        syntax: ['*', '?', '[0-9]', 'cp report-?.txt archive/'],
        concepts: ['globbing', 'wildcard expansion', 'character classes'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/archive"',
          'printf "one\\n" > "${student_home}/lab/report-1.txt"',
          'printf "two\\n" > "${student_home}/lab/report-2.txt"',
          'printf "three\\n" > "${student_home}/lab/report-3.txt"',
          'printf "keep out\\n" > "${student_home}/lab/report-final.txt"',
        ),
        tasks: [
          {
            instruction:
              'Copy only the three single-digit report files into /home/student/lab/archive. Do not copy report-final.txt.',
            solution:
              'cp /home/student/lab/report-[0-9].txt /home/student/lab/archive/',
            alternate: [
              'cd /home/student/lab && cp report-?.txt archive/',
            ],
            incorrect: 'cp /home/student/lab/report-*.txt /home/student/lab/archive/',
            validators: [
              entries(
                '/home/student/lab/archive',
                ['report-1.txt', 'report-2.txt', 'report-3.txt'],
                true,
              ),
            ],
            hints: [
              'A broad * also matches the word final.',
              '? matches exactly one character, and [0-9] matches one digit.',
              'Preview the pattern with ls before using it with cp.',
            ],
          },
        ],
      },
      {
        id: 'files.09',
        title: 'File Names with Spaces',
        purpose: 'Pass a spaced filename to a command as one argument.',
        mentalModel:
          'The shell normally splits unquoted whitespace into argument boundaries. Quotes or backslash escapes suppress that split.',
        commands: ['touch', 'rm'],
        syntax: ['touch "monthly report.txt"', 'rm monthly\\ report.txt'],
        concepts: ['quoting', 'escaping', 'arguments'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Create an empty file named monthly report.txt inside /home/student/lab. The space must be part of the filename.',
            solution: 'touch "/home/student/lab/monthly report.txt"',
            alternate: ['touch /home/student/lab/monthly\\ report.txt'],
            incorrect: 'touch /home/student/lab/monthly /home/student/lab/report.txt',
            validators: [
              file('/home/student/lab/monthly report.txt'),
              { type: 'file_size', path: '/home/student/lab/monthly report.txt', equals: 0 },
              missing('/home/student/lab/monthly'),
            ],
            hints: [
              'The shell must receive the whole path as one argument.',
              'Wrap the path in quotes or escape the space with a backslash.',
              'Check with ls -l before assuming two visible words mean two files.',
            ],
          },
        ],
      },
      {
        id: 'files.10',
        title: 'File Metadata',
        purpose: 'Inspect what a file is and the metadata the filesystem stores about it.',
        mentalModel:
          'A directory entry names an inode; stat reports inode metadata while file inspects content signatures to identify likely data type.',
        commands: ['stat', 'file'],
        syntax: ['stat FILE', 'file FILE', "stat -c '%n %s bytes' FILE"],
        concepts: ['inode metadata', 'file type', 'size'],
        setup: labSetup('printf "hello world\\n" > "${student_home}/lab/message.txt"'),
        tasks: [
          {
            instruction:
              'Use stat and file to create /home/student/answers/metadata.txt. It must record that message.txt is 12 bytes and identify it as text.',
            solution:
              "stat -c '%n %s bytes' /home/student/lab/message.txt > /home/student/answers/metadata.txt && file -b /home/student/lab/message.txt >> /home/student/answers/metadata.txt",
            alternate: [
              "printf '%s %s bytes\\n' /home/student/lab/message.txt \"$(stat -c %s /home/student/lab/message.txt)\" > /home/student/answers/metadata.txt; file -b /home/student/lab/message.txt >> /home/student/answers/metadata.txt",
            ],
            incorrect:
              "echo 'message.txt is unknown' > /home/student/answers/metadata.txt",
            validators: [
              file('/home/student/answers/metadata.txt'),
              contains('/home/student/answers/metadata.txt', '12 bytes'),
              contains('/home/student/answers/metadata.txt', 'text', {
                caseSensitive: false,
              }),
            ],
            hints: [
              'The task asks for both filesystem metadata and content-based type detection.',
              'Use stat for the byte count and file for the type.',
              'Append the second command so it does not overwrite the first result.',
            ],
          },
        ],
      },
      {
        id: 'files.11',
        title: 'Links',
        purpose: 'Create another filesystem name or a path reference to an existing file.',
        mentalModel:
          'A hard link is another name for the same inode. A symbolic link is a small object containing a target path and can become dangling.',
        commands: ['ln'],
        syntax: ['ln ORIGINAL HARD_LINK', 'ln -s TARGET SYMBOLIC_LINK'],
        concepts: ['inodes', 'hard links', 'symbolic links'],
        setup: labSetup('printf "shared data\\n" > "${student_home}/lab/original.txt"'),
        tasks: [
          {
            instruction:
              'Create /home/student/lab/hard-link as a hard link to original.txt and /home/student/lab/symbolic-link as a symbolic link to original.txt.',
            solution:
              'cd /home/student/lab && ln original.txt hard-link && ln -s original.txt symbolic-link',
            alternate: [
              'ln /home/student/lab/original.txt /home/student/lab/hard-link && ln -s /home/student/lab/original.txt /home/student/lab/symbolic-link',
            ],
            incorrect:
              'cp /home/student/lab/original.txt /home/student/lab/hard-link && cp /home/student/lab/original.txt /home/student/lab/symbolic-link',
            validators: [
              {
                type: 'hard_link_exists',
                path: '/home/student/lab/hard-link',
                linkTo: '/home/student/lab/original.txt',
              },
              {
                type: 'symbolic_link_exists',
                path: '/home/student/lab/symbolic-link',
                resolved: true,
                target: '/home/student/lab/original.txt',
              },
            ],
            hints: [
              'A copy has different data; the task asks for two kinds of links.',
              'ln creates a hard link by default, and -s creates a symbolic link.',
              'Using a relative symbolic target keeps the link readable inside the same directory.',
            ],
          },
        ],
      },
      {
        id: 'files.12',
        title: 'File Management Assessment',
        purpose: 'Organise a realistic project directory using the file-management tools together.',
        mentalModel:
          'Good file management is defined by the final tree: correct paths, preserved content, deliberate deletions and links that point where intended.',
        commands: ['mkdir', 'mv', 'cp', 'rm', 'ln'],
        syntax: ['mkdir -p DIRECTORY', 'mv SOURCE DESTINATION', 'ln -s TARGET LINK'],
        concepts: ['project organisation', 'safe deletion', 'links'],
        assessment: true,
        difficulty: 3,
        minutes: 18,
        setup: labSetup(
          'mkdir -p "${student_home}/lab/project/inbox"',
          'printf "port=8080\\n" > "${student_home}/lab/project/inbox/app.conf"',
          'printf "quarter one\\n" > "${student_home}/lab/project/inbox/draft-report.txt"',
          'printf "discard\\n" > "${student_home}/lab/project/inbox/cache.tmp"',
        ),
        tasks: [
          {
            kind: 'assessment',
            instruction:
              'Organise /home/student/lab/project: create config, reports and backup directories; move app.conf into config; rename draft-report.txt to reports/q1.txt; copy app.conf into backup; delete cache.tmp; and create latest-report as a symbolic link to reports/q1.txt.',
            solution:
              'cd /home/student/lab/project && mkdir -p config reports backup && mv inbox/app.conf config/app.conf && mv inbox/draft-report.txt reports/q1.txt && cp config/app.conf backup/app.conf && rm inbox/cache.tmp && ln -s reports/q1.txt latest-report',
            alternate: [
              'cd /home/student/lab/project && install -d config reports backup && install inbox/app.conf config/app.conf && mv inbox/draft-report.txt reports/q1.txt && cp config/app.conf backup/app.conf && rm inbox/app.conf inbox/cache.tmp && ln -s reports/q1.txt latest-report',
            ],
            incorrect:
              'rm -rf /home/student/lab/project/inbox && mkdir -p /home/student/lab/project/config',
            validators: [
              ...exact('/home/student/lab/project/config/app.conf', 'port=8080\n'),
              ...exact('/home/student/lab/project/backup/app.conf', 'port=8080\n'),
              ...exact('/home/student/lab/project/reports/q1.txt', 'quarter one\n'),
              missing('/home/student/lab/project/inbox/cache.tmp'),
              {
                type: 'symbolic_link_exists',
                path: '/home/student/lab/project/latest-report',
                resolved: true,
                target: '/home/student/lab/project/reports/q1.txt',
              },
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'reading-editing-text',
    number: 4,
    title: 'Reading and editing text',
    level: 'beginner',
    summary:
      'Read short and long text, select file beginnings and endings, count content, create precise text, and make safe edits with terminal editors.',
    outcomes: [
      'Choose cat, less, head or tail for the shape of the reading task',
      'Measure text with wc and create predictable output with echo and printf',
      'Create multiline files without accidental expansion',
      'Make and verify edits with nano or Vim',
    ],
    lessons: [
      {
        id: 'text.01',
        title: 'Displaying Files with cat',
        purpose: 'Send one or more complete text files to standard output.',
        mentalModel:
          'cat concatenates byte streams in the order named. It is ideal for short files and for joining inputs, but it has no paging controls.',
        commands: ['cat'],
        syntax: ['cat FILE', 'cat FILE1 FILE2'],
        concepts: ['standard output', 'concatenation'],
        setup: labSetup(
          'printf "alpha\\n" > "${student_home}/lab/part-1.txt"',
          'printf "beta\\n" > "${student_home}/lab/part-2.txt"',
        ),
        tasks: [
          {
            instruction:
              'Concatenate part-1.txt and part-2.txt, in that order, into /home/student/answers/combined.txt.',
            solution:
              'cat /home/student/lab/part-1.txt /home/student/lab/part-2.txt > /home/student/answers/combined.txt',
            alternate: [
              'cat /home/student/lab/part-{1,2}.txt > /home/student/answers/combined.txt',
            ],
            incorrect:
              'cat /home/student/lab/part-2.txt /home/student/lab/part-1.txt > /home/student/answers/combined.txt',
            validators: exact('/home/student/answers/combined.txt', 'alpha\nbeta\n'),
            hints: [
              'cat can receive more than one filename.',
              'Input order becomes output order.',
              'Redirect the combined stream into the answer file.',
            ],
          },
        ],
      },
      {
        id: 'text.02',
        title: 'Viewing Long Files with less',
        purpose: 'Read and search a long file without flooding terminal scrollback.',
        mentalModel:
          'less is a pager: it keeps the file separate, displays one screen at a time, and lets you search and move without modifying the source.',
        commands: ['less', 'grep'],
        syntax: ['less FILE', '/pattern', 'n', 'q'],
        concepts: ['pagers', 'interactive search'],
        setup: labSetup(
          'for number in $(seq 1 80); do printf "record %02d: routine\\n" "${number}"; done > "${student_home}/lab/operations.log"',
          'sed -i "47s/routine/release window at 22:00/" "${student_home}/lab/operations.log"',
        ),
        tasks: [
          {
            instruction:
              'Open operations.log with less, search for “release window”, then record the matching full line in /home/student/answers/less-finding.txt.',
            solution:
              "grep 'release window' /home/student/lab/operations.log > /home/student/answers/less-finding.txt",
            alternate: [
              "sed -n '/release window/p' /home/student/lab/operations.log > /home/student/answers/less-finding.txt",
            ],
            incorrect:
              "echo 'release window' > /home/student/answers/less-finding.txt",
            validators: exact(
              '/home/student/answers/less-finding.txt',
              'record 47: release window at 22:00\n',
            ),
            hints: [
              'Inside less, / begins a forward search and q exits.',
              'The saved answer must contain the whole matching line, not only the search phrase.',
              'After viewing, a filtering command can copy the matching line into the answer file.',
            ],
          },
        ],
      },
      {
        id: 'text.03',
        title: 'Beginning of Files with head',
        purpose: 'Inspect the first records of a file quickly.',
        mentalModel:
          'head reads from the start and stops after a requested number of lines, so it avoids processing or displaying the remainder.',
        commands: ['head'],
        syntax: ['head FILE', 'head -n 5 FILE'],
        concepts: ['file beginnings', 'line limits'],
        setup: labSetup(
          'for number in $(seq 1 12); do printf "item-%02d\\n" "${number}"; done > "${student_home}/lab/items.txt"',
        ),
        tasks: [
          {
            instruction:
              'Save exactly the first five lines of items.txt in /home/student/answers/first-five.txt.',
            solution:
              'head -n 5 /home/student/lab/items.txt > /home/student/answers/first-five.txt',
            alternate: [
              "sed -n '1,5p' /home/student/lab/items.txt > /home/student/answers/first-five.txt",
            ],
            incorrect:
              'tail -n 5 /home/student/lab/items.txt > /home/student/answers/first-five.txt',
            validators: exact(
              '/home/student/answers/first-five.txt',
              'item-01\nitem-02\nitem-03\nitem-04\nitem-05\n',
            ),
            hints: [
              'The needed records are at the beginning.',
              'head accepts a line count with -n.',
              'Redirect only those five lines into first-five.txt.',
            ],
          },
        ],
      },
      {
        id: 'text.04',
        title: 'End of Files with tail',
        purpose: 'Inspect the newest or final records in a file.',
        mentalModel:
          'tail starts near the end of a file. With -f it waits for appended data, which makes it useful for live logs.',
        commands: ['tail'],
        syntax: ['tail FILE', 'tail -n 4 FILE', 'tail -f application.log'],
        concepts: ['file endings', 'log following'],
        setup: labSetup(
          'for number in $(seq 1 12); do printf "event-%02d\\n" "${number}"; done > "${student_home}/lab/events.log"',
        ),
        tasks: [
          {
            instruction:
              'Save exactly the last four lines of events.log in /home/student/answers/last-four.txt.',
            solution:
              'tail -n 4 /home/student/lab/events.log > /home/student/answers/last-four.txt',
            alternate: [
              "sed -n '9,12p' /home/student/lab/events.log > /home/student/answers/last-four.txt",
            ],
            incorrect:
              'head -n 4 /home/student/lab/events.log > /home/student/answers/last-four.txt',
            validators: exact(
              '/home/student/answers/last-four.txt',
              'event-09\nevent-10\nevent-11\nevent-12\n',
            ),
            hints: [
              'The needed records are at the end.',
              'tail accepts a line count with -n.',
              'Request four lines, then redirect the output.',
            ],
          },
        ],
      },
      {
        id: 'text.05',
        title: 'Counting with wc',
        purpose: 'Measure lines, words and bytes in text.',
        mentalModel:
          'wc counts separators and bytes in an input stream. Its options select which measurements are printed; filenames may also appear in its output.',
        commands: ['wc'],
        syntax: ['wc FILE', 'wc -l FILE', 'wc -w FILE', 'wc -c FILE'],
        concepts: ['line counts', 'word counts', 'byte counts'],
        setup: labSetup('printf "red green\\nblue yellow\\n" > "${student_home}/lab/colours.txt"'),
        tasks: [
          {
            instruction:
              'Write the line count, word count and byte count of colours.txt as “2 4 22” in /home/student/answers/counts.txt, with no filename.',
            solution:
              "wc -l -w -c < /home/student/lab/colours.txt | awk '{print $1, $2, $3}' > /home/student/answers/counts.txt",
            alternate: [
              "printf '%s %s %s\\n' \"$(wc -l < /home/student/lab/colours.txt)\" \"$(wc -w < /home/student/lab/colours.txt)\" \"$(wc -c < /home/student/lab/colours.txt)\" | xargs > /home/student/answers/counts.txt",
            ],
            incorrect:
              'wc -l /home/student/lab/colours.txt > /home/student/answers/counts.txt',
            validators: exact('/home/student/answers/counts.txt', '2 4 22\n'),
            hints: [
              'wc can print all three requested counts in one run.',
              'Using input redirection avoids appending the filename.',
              'Normalise the spacing so the answer contains three numbers.',
            ],
          },
        ],
      },
      {
        id: 'text.06',
        title: 'Creating Text with echo',
        purpose: 'Produce a short line of text or show an expanded shell value.',
        mentalModel:
          'echo receives already-expanded shell arguments and writes them separated by spaces. Quoting controls expansion and argument boundaries before echo runs.',
        commands: ['echo'],
        syntax: ['echo "Hello"', 'echo "$HOME"'],
        concepts: ['shell expansion', 'short text output'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Create /home/student/answers/greeting.txt containing exactly “Hello from /home/student” on one line, using the HOME variable for the path.',
            solution:
              'echo "Hello from $HOME" > /home/student/answers/greeting.txt',
            alternate: [
              "printf 'Hello from %s\\n' \"$HOME\" > /home/student/answers/greeting.txt",
            ],
            incorrect:
              "echo 'Hello from $HOME' > /home/student/answers/greeting.txt",
            validators: exact(
              '/home/student/answers/greeting.txt',
              'Hello from /home/student\n',
            ),
            hints: [
              'The shell expands variables inside double quotes.',
              'Single quotes would preserve the dollar sign literally.',
              'Redirect the expanded echo output into greeting.txt.',
            ],
          },
        ],
      },
      {
        id: 'text.07',
        title: 'Creating Multiline Files',
        purpose: 'Create structured text containing several predictable lines.',
        mentalModel:
          'printf gives explicit control over newlines, while a here-document feeds a block of lines to a command until a chosen delimiter.',
        commands: ['printf', 'cat'],
        syntax: ["printf 'one\\ntwo\\n'", 'cat <<EOF'],
        concepts: ['newlines', 'here-documents', 'multiline text'],
        setup: labSetup(),
        tasks: [
          {
            instruction:
              'Create /home/student/answers/server.conf with exactly three lines: host=web1, port=8080 and mode=training.',
            solution:
              "printf 'host=web1\\nport=8080\\nmode=training\\n' > /home/student/answers/server.conf",
            alternate: [
              "cat > /home/student/answers/server.conf <<'EOF'\nhost=web1\nport=8080\nmode=training\nEOF",
            ],
            incorrect:
              "echo 'host=web1 port=8080 mode=training' > /home/student/answers/server.conf",
            validators: exact(
              '/home/student/answers/server.conf',
              'host=web1\nport=8080\nmode=training\n',
            ),
            hints: [
              'The values must occupy separate lines.',
              'printf recognises explicit newline escapes; a here-document preserves line breaks.',
              'Check the result with cat before submitting.',
            ],
          },
        ],
      },
      {
        id: 'text.08',
        title: 'Nano Basics',
        purpose: 'Make a small text edit with an approachable terminal editor.',
        mentalModel:
          'Nano edits a memory buffer and writes it back when you save. The shortcut bar uses ^ for Ctrl, so ^O writes and ^X exits.',
        commands: ['nano', 'grep'],
        syntax: ['nano FILE', 'Ctrl+O', 'Ctrl+X'],
        concepts: ['terminal editors', 'saving files'],
        setup: labSetup('printf "environment=staging\\n" > "${student_home}/lab/app.conf"'),
        tasks: [
          {
            instruction:
              'Open /home/student/lab/app.conf in nano, change staging to production, save, exit, and verify the line.',
            solution:
              "sed -i 's/environment=staging/environment=production/' /home/student/lab/app.conf",
            alternate: [
              "printf 'environment=production\\n' > /home/student/lab/app.conf",
            ],
            incorrect: "echo 'production' >> /home/student/lab/app.conf",
            validators: exact(
              '/home/student/lab/app.conf',
              'environment=production\n',
            ),
            hints: [
              'Nano shows its save and exit shortcuts at the bottom of the screen.',
              'Use Ctrl+O to write the buffer, then Ctrl+X to leave.',
              'Run grep production on the file after saving.',
            ],
          },
        ],
      },
      {
        id: 'text.09',
        title: 'Vim Survival Skills',
        purpose: 'Open, change, save and leave a file when Vim is the available editor.',
        mentalModel:
          'Vim is modal: normal mode interprets keys as commands, insert mode enters text, and colon commands such as :wq save and quit.',
        commands: ['vim', 'grep'],
        syntax: ['vim FILE', 'i', 'Esc', ':wq', '/pattern'],
        concepts: ['modal editing', 'normal mode', 'insert mode'],
        setup: labSetup('printf "status=draft\\n" > "${student_home}/lab/release.env"'),
        tasks: [
          {
            instruction:
              'Use Vim to change status=draft to status=final in /home/student/lab/release.env, then save and quit.',
            solution:
              "vim -Nu NONE -n -es -c 's/status=draft/status=final/' -c 'wq' /home/student/lab/release.env",
            alternate: [
              "sed -i 's/status=draft/status=final/' /home/student/lab/release.env",
            ],
            incorrect: "echo 'status=final' >> /home/student/lab/release.env",
            validators: exact('/home/student/lab/release.env', 'status=final\n'),
            hints: [
              'Press i to insert, and Esc to return to normal mode.',
              'A slash searches; :wq writes and quits.',
              'Verify the saved file after Vim closes.',
            ],
          },
        ],
      },
      {
        id: 'text.10',
        title: 'Text Editing Assessment',
        purpose: 'Repair and extend a configuration file, then prove the saved state is correct.',
        mentalModel:
          'An edit is complete only when the file on disk has the intended values, no stale conflicting line, and the required new setting.',
        commands: ['nano', 'vim', 'grep', 'printf'],
        syntax: ['nano FILE', 'vim FILE', 'grep PATTERN FILE'],
        concepts: ['configuration repair', 'verification'],
        assessment: true,
        difficulty: 3,
        minutes: 15,
        setup: labSetup(
          'printf "host=web1\\nport=not-a-number\\nmode=training\\n" > "${student_home}/lab/service.conf"',
        ),
        tasks: [
          {
            kind: 'assessment',
            instruction:
              'Edit /home/student/lab/service.conf so port is 8080, keep the existing host and mode, and add enabled=true as the final line.',
            solution:
              "sed -i 's/port=not-a-number/port=8080/' /home/student/lab/service.conf && printf 'enabled=true\\n' >> /home/student/lab/service.conf",
            alternate: [
              "printf 'host=web1\\nport=8080\\nmode=training\\nenabled=true\\n' > /home/student/lab/service.conf",
            ],
            incorrect:
              "echo 'port=8080' > /home/student/lab/service.conf",
            validators: exact(
              '/home/student/lab/service.conf',
              'host=web1\nport=8080\nmode=training\nenabled=true\n',
            ),
          },
        ],
      },
    ],
  },
  {
    id: 'search-filter-compare',
    number: 5,
    title: 'Searching, filtering, and comparing',
    level: 'foundation',
    summary:
      'Find relevant lines and files, filter by metadata, sort and count results, and compare versions without reading everything manually.',
    outcomes: [
      'Search text with grep, including case, inversion, recursion and context',
      'Find paths by name, type, size and modification time',
      'Sort, deduplicate and count text records',
      'Compare files and complete a small log-analysis assessment',
    ],
    lessons: [
      {
        id: 'search.01',
        title: 'Searching Text with grep',
        purpose: 'Select lines containing a word or phrase.',
        mentalModel:
          'grep tests each input line against a pattern and writes matching lines. It does not change the source file.',
        commands: ['grep'],
        syntax: ['grep PATTERN FILE', 'grep "connection failed" app.log'],
        concepts: ['line filtering', 'patterns'],
        setup: labSetup(
          'printf "INFO started\\nERROR disk full\\nINFO retry\\nERROR connection failed\\n" > "${student_home}/lab/app.log"',
        ),
        tasks: [
          {
            instruction:
              'Save only the ERROR lines from app.log to /home/student/answers/errors.txt.',
            solution:
              'grep ERROR /home/student/lab/app.log > /home/student/answers/errors.txt',
            alternate: [
              "sed -n '/ERROR/p' /home/student/lab/app.log > /home/student/answers/errors.txt",
            ],
            incorrect:
              'grep INFO /home/student/lab/app.log > /home/student/answers/errors.txt',
            validators: exact(
              '/home/student/answers/errors.txt',
              'ERROR disk full\nERROR connection failed\n',
            ),
            hints: [
              'Each wanted line shares the same marker.',
              'grep accepts the marker followed by the filename.',
              'Redirect the matching lines into errors.txt.',
            ],
          },
        ],
      },
      {
        id: 'search.02',
        title: 'Case-Insensitive and Inverted Search',
        purpose: 'Match text regardless of case and exclude unwanted lines.',
        mentalModel:
          '-i changes how grep compares letters; -v reverses the decision so nonmatching lines are selected.',
        commands: ['grep'],
        syntax: ['grep -i error FILE', 'grep -v success FILE'],
        concepts: ['case-insensitive matching', 'inverted matching'],
        setup: labSetup(
          'printf "SUCCESS boot\\nError disk\\nwarning retry\\nERROR network\\nsuccess stop\\n" > "${student_home}/lab/mixed.log"',
        ),
        tasks: [
          {
            instruction:
              'Find error lines in mixed.log regardless of case, exclude any line containing success regardless of case, and save the result to /home/student/answers/filtered.txt.',
            solution:
              'grep -i error /home/student/lab/mixed.log | grep -vi success > /home/student/answers/filtered.txt',
            alternate: [
              "awk 'tolower($0) ~ /error/ && tolower($0) !~ /success/' /home/student/lab/mixed.log > /home/student/answers/filtered.txt",
            ],
            incorrect:
              'grep error /home/student/lab/mixed.log > /home/student/answers/filtered.txt',
            validators: exact(
              '/home/student/answers/filtered.txt',
              'Error disk\nERROR network\n',
            ),
            hints: [
              'The file contains more than one capitalisation.',
              'Use -i for case-insensitive matching and -v for exclusion.',
              'A second grep stage can remove lines you do not want.',
            ],
          },
        ],
      },
      {
        id: 'search.03',
        title: 'Recursive Search',
        purpose: 'Find a setting across an unfamiliar directory tree.',
        mentalModel:
          'Recursive grep descends into directories and prefixes matches with filenames so you can locate the source as well as the text.',
        commands: ['grep', 'sort'],
        syntax: ['grep -R PATTERN DIRECTORY'],
        concepts: ['recursive search', 'configuration discovery'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/project/api" "${student_home}/lab/project/worker"',
          'printf "database_url=sqlite:///api.db\\n" > "${student_home}/lab/project/api/app.env"',
          'printf "threads=4\\n" > "${student_home}/lab/project/worker/worker.env"',
        ),
        tasks: [
          {
            instruction:
              'Recursively search /home/student/lab/project for database_url and save the match, including its filename, to /home/student/answers/database-setting.txt.',
            solution:
              'grep -R database_url /home/student/lab/project > /home/student/answers/database-setting.txt',
            alternate: [
              "find /home/student/lab/project -type f -exec grep -H database_url {} + > /home/student/answers/database-setting.txt",
            ],
            incorrect:
              'grep -R threads /home/student/lab/project > /home/student/answers/database-setting.txt',
            validators: [
              file('/home/student/answers/database-setting.txt'),
              contains('/home/student/answers/database-setting.txt', 'api/app.env'),
              contains(
                '/home/student/answers/database-setting.txt',
                'database_url=sqlite:///api.db',
              ),
              lineCount('/home/student/answers/database-setting.txt', 1),
            ],
            hints: [
              'The setting is somewhere below project, not necessarily in its top directory.',
              'grep -R descends recursively and reports the matching filename.',
              'Search for the setting name, not its guessed value.',
            ],
          },
        ],
      },
      {
        id: 'search.04',
        title: 'Line Numbers and Context',
        purpose: 'Show where a match occurred and the nearby records that explain it.',
        mentalModel:
          '-n annotates matches with line numbers; -A, -B and -C include surrounding lines without changing which line actually matched.',
        commands: ['grep'],
        syntax: ['grep -n PATTERN FILE', 'grep -C 1 PATTERN FILE'],
        concepts: ['line numbers', 'context lines'],
        setup: labSetup(
          'printf "start request\\nuser=student\\nERROR timeout\\nretry scheduled\\nrequest ended\\n" > "${student_home}/lab/request.log"',
        ),
        tasks: [
          {
            instruction:
              'Save the ERROR line from request.log with its line number and one line of context before and after to /home/student/answers/error-context.txt.',
            solution:
              'grep -n -C 1 ERROR /home/student/lab/request.log > /home/student/answers/error-context.txt',
            alternate: [
              "awk 'NR >= 2 && NR <= 4 {print NR \":\" $0}' /home/student/lab/request.log > /home/student/answers/error-context.txt",
            ],
            incorrect:
              'grep ERROR /home/student/lab/request.log > /home/student/answers/error-context.txt',
            validators: [
              file('/home/student/answers/error-context.txt'),
              contains('/home/student/answers/error-context.txt', '3:ERROR timeout'),
              contains('/home/student/answers/error-context.txt', 'user=student'),
              contains('/home/student/answers/error-context.txt', 'retry scheduled'),
            ],
            hints: [
              'The task needs both location and surrounding evidence.',
              'Use -n for line numbers and -C 1 for one line on each side.',
              'grep separates context lines with a hyphen after the line number.',
            ],
          },
        ],
      },
      {
        id: 'search.05',
        title: 'Finding Files by Name',
        purpose: 'Locate paths when you know a filename pattern but not the directory.',
        mentalModel:
          'find walks a directory tree and applies tests to each path. -name matches case-sensitively; -iname ignores case.',
        commands: ['find', 'sort'],
        syntax: ["find . -name '*.log'", "find . -iname '*error*'"],
        concepts: ['path search', 'name patterns'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/logs/archive" "${student_home}/lab/docs"',
          'touch "${student_home}/lab/logs/app.log" "${student_home}/lab/logs/archive/old.log" "${student_home}/lab/docs/notes.txt"',
        ),
        tasks: [
          {
            instruction:
              'Find every .log file below /home/student/lab, sort the absolute paths, and save them to /home/student/answers/log-files.txt.',
            solution:
              "find /home/student/lab -type f -name '*.log' | sort > /home/student/answers/log-files.txt",
            alternate: [
              "find /home/student/lab -name '*.log' -type f -print | sort > /home/student/answers/log-files.txt",
            ],
            incorrect:
              "find /home/student/lab -name '*.txt' > /home/student/answers/log-files.txt",
            validators: exact(
              '/home/student/answers/log-files.txt',
              '/home/student/lab/logs/app.log\n/home/student/lab/logs/archive/old.log\n',
            ),
            hints: [
              'Start the search at the common parent directory.',
              'Combine -type f with a quoted -name pattern.',
              'Sort the resulting absolute paths before saving them.',
            ],
          },
        ],
      },
      {
        id: 'search.06',
        title: 'Finding by Type and Size',
        purpose: 'Select filesystem objects by metadata rather than by name.',
        mentalModel:
          'find tests such as -type and -size are combined with logical AND by default, so every listed condition must hold.',
        commands: ['find'],
        syntax: ['find . -type f', 'find . -type d', 'find . -size +1M'],
        concepts: ['file type', 'file size', 'combined tests'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/data/subdir"',
          'printf "small\\n" > "${student_home}/lab/data/small.dat"',
          'dd if=/dev/zero of="${student_home}/lab/data/large.dat" bs=1M count=2 status=none',
        ),
        tasks: [
          {
            instruction:
              'Find regular files larger than 1 MiB below /home/student/lab/data and save their absolute paths to /home/student/answers/large-files.txt.',
            solution:
              'find /home/student/lab/data -type f -size +1M > /home/student/answers/large-files.txt',
            alternate: [
              "find /home/student/lab/data -type f -size +1048576c -print > /home/student/answers/large-files.txt",
            ],
            incorrect:
              'find /home/student/lab/data -type d > /home/student/answers/large-files.txt',
            validators: exact(
              '/home/student/answers/large-files.txt',
              '/home/student/lab/data/large.dat\n',
            ),
            hints: [
              'The task combines an object type and a size threshold.',
              'Use -type f and -size +1M.',
              'The plus sign means strictly larger than the given unit.',
            ],
          },
        ],
      },
      {
        id: 'search.07',
        title: 'Finding by Time',
        purpose: 'Locate files changed within a useful time window.',
        mentalModel:
          'find compares stored timestamps to age tests. -mtime uses rounded 24-hour periods; -mmin gives minute-level windows.',
        commands: ['find'],
        syntax: ['find . -mtime -1', 'find . -mmin -30'],
        concepts: ['modification time', 'age tests'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/time"',
          'touch -d "2 days ago" "${student_home}/lab/time/old.log"',
          'touch -d "5 minutes ago" "${student_home}/lab/time/recent.log"',
        ),
        tasks: [
          {
            instruction:
              'Find files modified within the last 30 minutes in /home/student/lab/time and save only their names to /home/student/answers/recent-files.txt.',
            solution:
              "find /home/student/lab/time -type f -mmin -30 -printf '%f\\n' > /home/student/answers/recent-files.txt",
            alternate: [
              "find /home/student/lab/time -type f -newermt '30 minutes ago' -printf '%f\\n' > /home/student/answers/recent-files.txt",
            ],
            incorrect:
              "find /home/student/lab/time -type f -mmin +30 -printf '%f\\n' > /home/student/answers/recent-files.txt",
            validators: exact(
              '/home/student/answers/recent-files.txt',
              'recent.log\n',
            ),
            hints: [
              'Use minute precision rather than rounded days.',
              '-mmin -30 means younger than 30 minutes.',
              '-printf can emit only the filename component.',
            ],
          },
        ],
      },
      {
        id: 'search.08',
        title: 'Running Actions with find',
        purpose: 'Preview a path selection and then apply a controlled action to every match.',
        mentalModel:
          'find first selects paths and then runs an action such as -print, -delete or -exec. Put tests before destructive actions and preview the same expression.',
        commands: ['find', 'rm'],
        syntax: ["find . -name '*.tmp' -print", "find . -name '*.tmp' -delete"],
        concepts: ['find actions', 'safe preview', 'batch deletion'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/cache/nested"',
          'touch "${student_home}/lab/cache/a.tmp" "${student_home}/lab/cache/nested/b.tmp"',
          'printf "keep\\n" > "${student_home}/lab/cache/keep.txt"',
        ),
        tasks: [
          {
            instruction:
              'Delete every .tmp file below /home/student/lab/cache with find, while preserving keep.txt and the directory tree.',
            solution:
              "find /home/student/lab/cache -type f -name '*.tmp' -delete",
            alternate: [
              "find /home/student/lab/cache -type f -name '*.tmp' -exec rm -- {} +",
            ],
            incorrect: 'rm -rf /home/student/lab/cache',
            validators: [
              missing('/home/student/lab/cache/a.tmp'),
              missing('/home/student/lab/cache/nested/b.tmp'),
              ...exact('/home/student/lab/cache/keep.txt', 'keep\n'),
              directory('/home/student/lab/cache/nested'),
            ],
            hints: [
              'Preview the exact find expression with -print first.',
              'After confirming, replace -print with -delete.',
              'Keep -type f so directories are never selected.',
            ],
          },
        ],
      },
      {
        id: 'search.09',
        title: 'Sorting Text',
        purpose: 'Put text records into a predictable lexical or numeric order.',
        mentalModel:
          'sort compares whole lines by default. -n compares numeric value, and -r reverses the chosen order.',
        commands: ['sort'],
        syntax: ['sort FILE', 'sort -n FILE', 'sort -r FILE'],
        concepts: ['lexical order', 'numeric order', 'reverse order'],
        setup: labSetup('printf "20\\n3\\n100\\n11\\n" > "${student_home}/lab/numbers.txt"'),
        tasks: [
          {
            instruction:
              'Sort numbers.txt numerically from largest to smallest and save the result to /home/student/answers/numbers-sorted.txt.',
            solution:
              'sort -nr /home/student/lab/numbers.txt > /home/student/answers/numbers-sorted.txt',
            alternate: [
              'sort -n /home/student/lab/numbers.txt | tac > /home/student/answers/numbers-sorted.txt',
            ],
            incorrect:
              'sort -r /home/student/lab/numbers.txt > /home/student/answers/numbers-sorted.txt',
            validators: exact(
              '/home/student/answers/numbers-sorted.txt',
              '100\n20\n11\n3\n',
            ),
            hints: [
              'Text order and numeric order are different for 100 and 20.',
              'Use -n for numbers and -r for descending order.',
              'The options can be grouped as -nr.',
            ],
          },
        ],
      },
      {
        id: 'search.10',
        title: 'Removing Duplicates',
        purpose: 'Collapse repeated records and count how often each one occurs.',
        mentalModel:
          'uniq only compares adjacent lines, so unsorted input must usually pass through sort first. -c prefixes each group with its count.',
        commands: ['sort', 'uniq', 'awk'],
        syntax: ['sort FILE | uniq', 'sort FILE | uniq -c'],
        concepts: ['adjacent duplicates', 'frequency counts'],
        setup: labSetup('printf "api\\nweb\\napi\\ndb\\nweb\\napi\\n" > "${student_home}/lab/services.txt"'),
        tasks: [
          {
            instruction:
              'Count each unique service name and save “3 api”, “1 db”, “2 web” as sorted lines in /home/student/answers/service-counts.txt.',
            solution:
              "sort /home/student/lab/services.txt | uniq -c | awk '{print $1, $2}' > /home/student/answers/service-counts.txt",
            alternate: [
              "awk '{count[$0]++} END {for (name in count) print count[name], name}' /home/student/lab/services.txt | sort -k2 > /home/student/answers/service-counts.txt",
            ],
            incorrect:
              'uniq -c /home/student/lab/services.txt > /home/student/answers/service-counts.txt',
            validators: exact(
              '/home/student/answers/service-counts.txt',
              '3 api\n1 db\n2 web\n',
            ),
            hints: [
              'Repeated names are not adjacent in the source.',
              'Sort first, then use uniq -c.',
              'awk can normalise the leading spaces in the count output.',
            ],
          },
        ],
      },
      {
        id: 'search.11',
        title: 'Comparing Files',
        purpose: 'See whether two files differ and identify the changed lines.',
        mentalModel:
          'cmp answers whether bytes differ and where the first difference occurs; diff produces a line-oriented description useful for text.',
        commands: ['diff', 'cmp'],
        syntax: ['diff OLD NEW', 'cmp FILE1 FILE2'],
        concepts: ['text differences', 'byte comparison'],
        setup: labSetup(
          'printf "host=web1\\nport=8080\\n" > "${student_home}/lab/old.conf"',
          'printf "host=web1\\nport=9090\\n" > "${student_home}/lab/new.conf"',
        ),
        tasks: [
          {
            instruction:
              'Compare old.conf with new.conf and save a normal diff showing both port values to /home/student/answers/config.diff.',
            solution:
              'diff /home/student/lab/old.conf /home/student/lab/new.conf > /home/student/answers/config.diff || true',
            alternate: [
              'diff --normal /home/student/lab/old.conf /home/student/lab/new.conf > /home/student/answers/config.diff || true',
            ],
            incorrect:
              'cmp /home/student/lab/old.conf /home/student/lab/new.conf > /home/student/answers/config.diff || true',
            validators: [
              file('/home/student/answers/config.diff'),
              contains('/home/student/answers/config.diff', '< port=8080'),
              contains('/home/student/answers/config.diff', '> port=9090'),
            ],
            hints: [
              'The answer must show changed text lines, not only a byte position.',
              'Use diff with the old file first and the new file second.',
              'diff uses exit code 1 for ordinary differences, which is not a command failure.',
            ],
          },
        ],
      },
      {
        id: 'search.12',
        title: 'Search and Analysis Assessment',
        purpose: 'Combine searching, counting, sorting and comparison in a small incident report.',
        mentalModel:
          'A useful analysis leaves reproducible evidence: a count, a ranked result and a diff derived from source files rather than guessed.',
        commands: ['grep', 'sort', 'uniq', 'awk', 'diff'],
        syntax: ['grep PATTERN FILE', 'sort | uniq -c', 'diff OLD NEW'],
        concepts: ['log analysis', 'frequency ranking', 'configuration drift'],
        assessment: true,
        difficulty: 4,
        minutes: 20,
        setup: labSetup(
          'printf "INFO start\\nERROR disk\\nERROR auth\\nINFO retry\\nERROR disk\\n" > "${student_home}/lab/app.log"',
          'printf "port=8080\\nmode=prod\\n" > "${student_home}/lab/old.conf"',
          'printf "port=9090\\nmode=prod\\n" > "${student_home}/lab/new.conf"',
        ),
        tasks: [
          {
            kind: 'assessment',
            instruction:
              'Create three evidence files: error-count.txt containing the number of ERROR lines, common-error.txt containing only the most common error word, and config.diff showing the change between old.conf and new.conf.',
            solution:
              "grep -c '^ERROR' /home/student/lab/app.log > /home/student/answers/error-count.txt && grep '^ERROR' /home/student/lab/app.log | awk '{print $2}' | sort | uniq -c | sort -nr | awk 'NR==1 {print $2}' > /home/student/answers/common-error.txt; diff /home/student/lab/old.conf /home/student/lab/new.conf > /home/student/answers/config.diff || true",
            alternate: [
              "awk '/^ERROR/{count++; errors[$2]++} END{print count > \"/home/student/answers/error-count.txt\"; for (name in errors) if (errors[name] > max) {max=errors[name]; common=name} print common > \"/home/student/answers/common-error.txt\"}' /home/student/lab/app.log; diff /home/student/lab/old.conf /home/student/lab/new.conf > /home/student/answers/config.diff || true",
            ],
            incorrect:
              "echo 1 > /home/student/answers/error-count.txt; echo auth > /home/student/answers/common-error.txt; touch /home/student/answers/config.diff",
            validators: [
              ...exact('/home/student/answers/error-count.txt', '3\n'),
              ...exact('/home/student/answers/common-error.txt', 'disk\n'),
              contains('/home/student/answers/config.diff', '< port=8080'),
              contains('/home/student/answers/config.diff', '> port=9090'),
            ],
          },
        ],
      },
    ],
  },
  {
    id: 'pipes-redirection-streams',
    number: 6,
    title: 'Pipes, redirection, and streams',
    level: 'foundation',
    summary:
      'Connect commands through standard streams, redirect normal output and errors deliberately, and build reliable multi-stage text-processing pipelines.',
    outcomes: [
      'Distinguish standard input, standard output and standard error',
      'Create, overwrite and append files with redirection',
      'Build and inspect multi-stage pipelines',
      'Use tee, xargs and process substitution safely',
      'Produce a ranked incident report while preserving diagnostic errors',
    ],
    lessons: [
      {
        id: 'streams.01',
        title: 'Standard Input, Output, and Error',
        purpose: 'Treat normal results and diagnostic messages as separate data streams.',
        mentalModel:
          'Every process starts with file descriptors 0, 1 and 2 for standard input, output and error. The shell can connect each descriptor independently.',
        commands: ['bash'],
        syntax: ['stdin = 0', 'stdout = 1', 'stderr = 2'],
        concepts: ['file descriptors', 'standard streams'],
        setup: labSetup(
          'cat > "${student_home}/lab/stream-demo" <<\'SCRIPT\'',
          '#!/usr/bin/env bash',
          'printf "normal message\\n"',
          'printf "error message\\n" >&2',
          'SCRIPT',
          'chmod 0755 "${student_home}/lab/stream-demo"',
        ),
        tasks: [
          {
            instruction:
              'Run stream-demo, save stdout in /home/student/answers/stdout.txt and stderr in /home/student/answers/stderr.txt.',
            solution:
              '/home/student/lab/stream-demo > /home/student/answers/stdout.txt 2> /home/student/answers/stderr.txt',
            alternate: [
              '{ /home/student/lab/stream-demo 2> /home/student/answers/stderr.txt; } > /home/student/answers/stdout.txt',
            ],
            incorrect:
              '/home/student/lab/stream-demo > /home/student/answers/stdout.txt',
            validators: [
              ...exact('/home/student/answers/stdout.txt', 'normal message\n'),
              ...exact('/home/student/answers/stderr.txt', 'error message\n'),
            ],
            hints: [
              'Normal output and errors use different numbered descriptors.',
              '> redirects descriptor 1; 2> redirects descriptor 2.',
              'Apply both redirections to the same command.',
            ],
          },
        ],
      },
      {
        id: 'streams.02',
        title: 'Redirecting Output',
        purpose: 'Save command output in a file, replacing an older file when that is intended.',
        mentalModel:
          '> asks the shell to open the destination for writing and truncate it before the command starts. The command itself does not know a file is involved.',
        commands: ['echo'],
        syntax: ['echo "report" > report.txt'],
        concepts: ['stdout redirection', 'file truncation'],
        setup: labSetup('printf "old report\\n" > "${student_home}/answers/report.txt"'),
        tasks: [
          {
            instruction:
              'Replace the existing contents of /home/student/answers/report.txt with exactly “new report”.',
            solution: "echo 'new report' > /home/student/answers/report.txt",
            alternate: [
              "printf 'new report\\n' > /home/student/answers/report.txt",
            ],
            incorrect: "echo 'new report' >> /home/student/answers/report.txt",
            validators: exact('/home/student/answers/report.txt', 'new report\n'),
            hints: [
              'The old line must disappear.',
              'A single > truncates before writing.',
              'Use >> only when the old contents must remain.',
            ],
          },
        ],
      },
      {
        id: 'streams.03',
        title: 'Appending Output',
        purpose: 'Add new output to the end of an existing file.',
        mentalModel:
          '>> opens the destination in append mode, so each write goes after existing bytes instead of truncating them.',
        commands: ['echo'],
        syntax: ['echo "second line" >> report.txt'],
        concepts: ['append redirection', 'preserving content'],
        setup: labSetup('printf "first line\\n" > "${student_home}/answers/report.txt"'),
        tasks: [
          {
            instruction:
              'Append “second line” to report.txt without changing its existing first line.',
            solution: "echo 'second line' >> /home/student/answers/report.txt",
            alternate: [
              "printf 'second line\\n' >> /home/student/answers/report.txt",
            ],
            incorrect: "echo 'second line' > /home/student/answers/report.txt",
            validators: exact(
              '/home/student/answers/report.txt',
              'first line\nsecond line\n',
            ),
            hints: [
              'The existing bytes must remain.',
              'Use the double redirect operator >>.',
              'Check both lines with cat after appending.',
            ],
          },
        ],
      },
      {
        id: 'streams.04',
        title: 'Redirecting Input',
        purpose: 'Provide a file as a command’s standard input.',
        mentalModel:
          '< asks the shell to connect descriptor 0 to a file. This is useful for commands that read stdin and avoids a filename in their output.',
        commands: ['sort'],
        syntax: ['sort < names.txt'],
        concepts: ['stdin redirection', 'input streams'],
        setup: labSetup('printf "zara\\nanna\\nmika\\n" > "${student_home}/lab/names.txt"'),
        tasks: [
          {
            instruction:
              'Sort names.txt using input redirection and save the result in /home/student/answers/names-sorted.txt.',
            solution:
              'sort < /home/student/lab/names.txt > /home/student/answers/names-sorted.txt',
            alternate: [
              'sort /home/student/lab/names.txt > /home/student/answers/names-sorted.txt',
            ],
            incorrect:
              'cat /home/student/lab/names.txt > /home/student/answers/names-sorted.txt',
            validators: exact(
              '/home/student/answers/names-sorted.txt',
              'anna\nmika\nzara\n',
            ),
            hints: [
              'The command can read the data through descriptor 0.',
              'Place < and the input file before the output redirection.',
              'Use a separate > for the sorted result.',
            ],
          },
        ],
      },
      {
        id: 'streams.05',
        title: 'Redirecting Errors',
        purpose: 'Capture diagnostics without mixing them into normal results.',
        mentalModel:
          'Errors conventionally use descriptor 2, so redirecting stdout alone does not catch them. 2> names the error descriptor explicitly.',
        commands: ['bash'],
        syntax: ['command 2> errors.txt'],
        concepts: ['stderr redirection', 'diagnostics'],
        setup: labSetup(
          'cat > "${student_home}/lab/stream-demo" <<\'SCRIPT\'',
          '#!/usr/bin/env bash',
          'printf "usable result\\n"',
          'printf "permission denied in sample\\n" >&2',
          'SCRIPT',
          'chmod 0755 "${student_home}/lab/stream-demo"',
        ),
        tasks: [
          {
            instruction:
              'Run stream-demo and save only its diagnostic stream in /home/student/answers/errors.txt.',
            solution:
              '/home/student/lab/stream-demo 2> /home/student/answers/errors.txt > /dev/null',
            alternate: [
              '/home/student/lab/stream-demo >/dev/null 2>/home/student/answers/errors.txt',
            ],
            incorrect:
              '/home/student/lab/stream-demo > /home/student/answers/errors.txt',
            validators: exact(
              '/home/student/answers/errors.txt',
              'permission denied in sample\n',
            ),
            hints: [
              'The diagnostic is not on normal stdout.',
              'Prefix the output operator with descriptor number 2.',
              'Discard ordinary output in /dev/null if you only need errors.',
            ],
          },
        ],
      },
      {
        id: 'streams.06',
        title: 'Combining Output and Errors',
        purpose: 'Create one chronological log containing both result and diagnostic streams.',
        mentalModel:
          '2>&1 makes descriptor 2 point wherever descriptor 1 points at that moment, so redirection order changes the outcome.',
        commands: ['bash'],
        syntax: ['command > output.txt 2>&1', 'command &> output.txt'],
        concepts: ['descriptor duplication', 'redirection order'],
        setup: labSetup(
          'cat > "${student_home}/lab/stream-demo" <<\'SCRIPT\'',
          '#!/usr/bin/env bash',
          'printf "normal message\\n"',
          'printf "error message\\n" >&2',
          'SCRIPT',
          'chmod 0755 "${student_home}/lab/stream-demo"',
        ),
        tasks: [
          {
            instruction:
              'Run stream-demo and combine both streams in /home/student/answers/combined.log.',
            solution:
              '/home/student/lab/stream-demo > /home/student/answers/combined.log 2>&1',
            alternate: [
              '/home/student/lab/stream-demo &> /home/student/answers/combined.log',
            ],
            incorrect:
              '/home/student/lab/stream-demo 2> /home/student/answers/combined.log',
            validators: exact(
              '/home/student/answers/combined.log',
              'normal message\nerror message\n',
            ),
            hints: [
              'Both descriptor 1 and descriptor 2 must reach the same file.',
              'Redirect stdout first, then duplicate stderr to it with 2>&1.',
              'Bash also supports the shorthand &>.',
            ],
          },
        ],
      },
      {
        id: 'streams.07',
        title: 'Pipes',
        purpose: 'Feed one command’s output directly into another command’s input.',
        mentalModel:
          '| connects stdout on the left to stdin on the right. No intermediate file is needed, and stderr remains separate unless redirected.',
        commands: ['grep', 'cat'],
        syntax: ['cat app.log | grep ERROR', 'grep ERROR app.log'],
        concepts: ['pipelines', 'stream connections'],
        setup: labSetup(
          'printf "INFO start\\nERROR disk\\nINFO retry\\nERROR auth\\n" > "${student_home}/lab/app.log"',
        ),
        tasks: [
          {
            instruction:
              'Use a pipeline to save only ERROR lines from app.log in /home/student/answers/errors.txt.',
            solution:
              'cat /home/student/lab/app.log | grep ERROR > /home/student/answers/errors.txt',
            alternate: [
              'grep ERROR /home/student/lab/app.log > /home/student/answers/errors.txt',
            ],
            incorrect:
              'cat /home/student/lab/app.log > /home/student/answers/errors.txt',
            validators: exact(
              '/home/student/answers/errors.txt',
              'ERROR disk\nERROR auth\n',
            ),
            hints: [
              'The left command produces all lines and the right command selects some.',
              'Place | between cat and grep.',
              'For a single file, grep can also read the file directly.',
            ],
          },
        ],
      },
      {
        id: 'streams.08',
        title: 'Multi-Stage Pipelines',
        purpose: 'Transform a stream through several small, composable stages.',
        mentalModel:
          'Each pipeline stage should do one clear transformation. Intermediate output can be inspected by running the pipeline only up to that stage.',
        commands: ['grep', 'awk', 'sort', 'uniq'],
        syntax: ['grep ERROR app.log | sort | uniq -c | sort -nr'],
        concepts: ['pipeline stages', 'frequency analysis'],
        setup: labSetup(
          'printf "ERROR disk\\nINFO start\\nERROR auth\\nERROR disk\\nERROR disk\\nERROR auth\\n" > "${student_home}/lab/app.log"',
        ),
        tasks: [
          {
            instruction:
              'Create /home/student/answers/error-counts.txt containing “3 disk” then “2 auth”, using a multi-stage pipeline.',
            solution:
              "grep '^ERROR' /home/student/lab/app.log | awk '{print $2}' | sort | uniq -c | sort -nr | awk '{print $1, $2}' > /home/student/answers/error-counts.txt",
            alternate: [
              "awk '/^ERROR/{count[$2]++} END{for (name in count) print count[name], name}' /home/student/lab/app.log | sort -nr > /home/student/answers/error-counts.txt",
            ],
            incorrect:
              "grep '^ERROR' /home/student/lab/app.log > /home/student/answers/error-counts.txt",
            validators: exact(
              '/home/student/answers/error-counts.txt',
              '3 disk\n2 auth\n',
            ),
            hints: [
              'First isolate errors, then isolate the error name.',
              'sort makes identical names adjacent so uniq -c can count them.',
              'A final numeric reverse sort puts the largest count first.',
            ],
          },
        ],
      },
      {
        id: 'streams.09',
        title: 'tee',
        purpose: 'Save a stream and continue passing the same data through a pipeline.',
        mentalModel:
          'tee copies stdin to each named file and to stdout. It is a T-junction, useful when you need both evidence and continued processing.',
        commands: ['grep', 'tee'],
        syntax: ['grep ERROR app.log | tee errors.txt'],
        concepts: ['stream duplication', 'pipeline evidence'],
        setup: labSetup(
          'printf "INFO start\\nERROR disk\\nERROR auth\\n" > "${student_home}/lab/app.log"',
        ),
        tasks: [
          {
            instruction:
              'Filter ERROR lines from app.log, use tee to save them in /home/student/answers/errors.txt, and send tee’s displayed copy to /dev/null.',
            solution:
              'grep ERROR /home/student/lab/app.log | tee /home/student/answers/errors.txt > /dev/null',
            alternate: [
              'grep ERROR /home/student/lab/app.log | tee /home/student/answers/errors.txt >/dev/null',
            ],
            incorrect:
              'grep INFO /home/student/lab/app.log | tee /home/student/answers/errors.txt > /dev/null',
            validators: exact(
              '/home/student/answers/errors.txt',
              'ERROR disk\nERROR auth\n',
            ),
            hints: [
              'tee reads the filtered stream, not the original file.',
              'Place tee after grep in the pipeline.',
              'Redirect tee’s stdout to /dev/null; the named file still receives a copy.',
            ],
          },
        ],
      },
      {
        id: 'streams.10',
        title: 'xargs',
        purpose: 'Turn streamed records into arguments for another command.',
        mentalModel:
          'xargs groups input records into command arguments. NUL-delimited input and -0 preserve spaces, quotes and newlines in filenames safely.',
        commands: ['find', 'xargs', 'rm'],
        syntax: ["find . -name '*.tmp' -print0 | xargs -0 rm"],
        concepts: ['argument construction', 'NUL delimiters', 'safe filenames'],
        setup: labSetup(
          'mkdir -p "${student_home}/lab/cache"',
          'touch "${student_home}/lab/cache/a.tmp" "${student_home}/lab/cache/old item.tmp"',
          'printf "keep\\n" > "${student_home}/lab/cache/keep.txt"',
        ),
        tasks: [
          {
            instruction:
              'Use find -print0 and xargs -0 to remove both .tmp files from cache, including the filename with a space, while preserving keep.txt.',
            solution:
              "find /home/student/lab/cache -type f -name '*.tmp' -print0 | xargs -0 rm --",
            alternate: [
              "find /home/student/lab/cache -type f -name '*.tmp' -delete",
            ],
            incorrect:
              "find /home/student/lab/cache -type f -name '*.tmp' -print | xargs rm",
            validators: [
              entries('/home/student/lab/cache', ['keep.txt'], true),
              ...exact('/home/student/lab/cache/keep.txt', 'keep\n'),
            ],
            hints: [
              'Whitespace-delimited filenames would split old item.tmp incorrectly.',
              'Use -print0 on find and matching -0 on xargs.',
              'Pass -- to rm before the generated paths.',
            ],
          },
        ],
      },
      {
        id: 'streams.11',
        title: 'Process Substitution',
        purpose: 'Give a command temporary file-like views of generated output.',
        mentalModel:
          '<(command) runs a command and expands to a readable path for its output, so file-oriented tools such as diff can compare pipelines.',
        commands: ['diff', 'sort'],
        syntax: ['diff <(sort file1) <(sort file2)'],
        concepts: ['process substitution', 'temporary streams'],
        setup: labSetup(
          'printf "beta\\nalpha\\ngamma\\n" > "${student_home}/lab/list-1.txt"',
          'printf "gamma\\nbeta\\nalpha\\n" > "${student_home}/lab/list-2.txt"',
        ),
        tasks: [
          {
            instruction:
              'Use process substitution to compare sorted list-1.txt and list-2.txt, saving the empty diff in /home/student/answers/differences.txt.',
            solution:
              'diff <(sort /home/student/lab/list-1.txt) <(sort /home/student/lab/list-2.txt) > /home/student/answers/differences.txt',
            alternate: [
              'comm -3 <(sort /home/student/lab/list-1.txt) <(sort /home/student/lab/list-2.txt) > /home/student/answers/differences.txt',
            ],
            incorrect:
              'diff /home/student/lab/list-1.txt /home/student/lab/list-2.txt > /home/student/answers/differences.txt || true',
            validators: [
              file('/home/student/answers/differences.txt'),
              {
                type: 'file_size',
                path: '/home/student/answers/differences.txt',
                equals: 0,
              },
            ],
            hints: [
              'The files contain the same records in different orders.',
              'Sort each file inside its own <(...) expression.',
              'diff receives the two generated paths as ordinary arguments.',
            ],
          },
        ],
      },
      {
        id: 'streams.12',
        title: 'Pipeline Assessment',
        purpose: 'Build a ranked security report and keep diagnostic noise separate.',
        mentalModel:
          'A production pipeline makes its data path explicit: select, extract, sort, count and rank normal records while redirecting errors to their own evidence file.',
        commands: ['grep', 'awk', 'sort', 'uniq', 'find'],
        syntax: ['grep | awk | sort | uniq -c | sort -nr', 'command 2> errors.txt'],
        concepts: ['log pipelines', 'ranked counts', 'separate diagnostics'],
        assessment: true,
        difficulty: 4,
        minutes: 22,
        setup: labSetup(
          'cat > "${student_home}/lab/auth.log" <<\'LOG\'',
          'Failed password for invalid user admin from 10.20.0.5',
          'Accepted publickey for student from 10.20.0.8',
          'Failed password for root from 10.20.0.9',
          'Failed password for invalid user admin from 10.20.0.5',
          'LOG',
        ),
        tasks: [
          {
            kind: 'assessment',
            instruction:
              'Create /home/student/answers/failed-users.txt containing “2 admin” and “1 root”, highest count first. Also run a search of /root as student and save its diagnostics separately in /home/student/answers/search-errors.txt.',
            solution:
              "grep 'Failed password' /home/student/lab/auth.log | awk '{if ($4==\"invalid\") print $6; else print $4}' | sort | uniq -c | sort -nr | awk '{print $1, $2}' > /home/student/answers/failed-users.txt; LC_ALL=C find /root -name definitely-absent > /dev/null 2> /home/student/answers/search-errors.txt || true",
            alternate: [
              "awk '/Failed password/{if ($4==\"invalid\") count[$6]++; else count[$4]++} END{for (user in count) print count[user], user}' /home/student/lab/auth.log | sort -nr > /home/student/answers/failed-users.txt; LC_ALL=C ls /root > /dev/null 2> /home/student/answers/search-errors.txt || true",
            ],
            incorrect:
              "printf '1 admin\\n1 root\\n' > /home/student/answers/failed-users.txt; touch /home/student/answers/search-errors.txt",
            validators: [
              ...exact(
                '/home/student/answers/failed-users.txt',
                '2 admin\n1 root\n',
              ),
              file('/home/student/answers/search-errors.txt'),
              contains(
                '/home/student/answers/search-errors.txt',
                'Permission denied',
                { caseSensitive: false },
              ),
            ],
          },
        ],
      },
    ],
  },
];

function lessonJson(
  module: ModuleSeed,
  seed: LessonSeed,
  prerequisite: string,
): Record<string, unknown> {
  const assessment = seed.assessment === true;
  const primarySyntax = seed.syntax[0] ?? seed.commands[0];
  return {
    schemaVersion: 1,
    id: seed.id,
    title: seed.title,
    level: module.level,
    module: module.id,
    type: assessment ? 'assessment' : 'guided-practice',
    estimatedDifficulty: seed.difficulty ?? (assessment ? 3 : 2),
    estimatedMinutes: seed.minutes ?? (assessment ? 15 : 9),
    prerequisites: [prerequisite],
    concepts: seed.concepts,
    commands: seed.commands,
    environment: {
      profile: 'beginner-core',
      resetPolicy: 'per-attempt',
      networkMode: 'disabled',
      sudoAllowed: false,
      ...(seed.setup ? { setupScript: 'setup.sh', resetScript: 'setup.sh' } : {}),
    },
    content: {
      purpose: seed.purpose,
      mentalModel: seed.mentalModel,
      syntax: assessment ? [] : seed.syntax,
      demonstration: assessment
        ? []
        : [
            {
              command: primarySyntax,
              explanation: `This small example isolates the main idea behind ${seed.title.toLowerCase()}.`,
            },
          ],
      summary: {
        remember: [seed.mentalModel, `Verify the resulting state before moving on.`],
        related: seed.commands,
      },
    },
    tasks: seed.tasks.map((task, index) => ({
      id: `task-${index + 1}`,
      kind: task.kind ?? (assessment ? 'assessment' : index === 0 ? 'guided' : 'independent'),
      instruction: task.instruction,
      ...(task.context ? { context: task.context } : {}),
      validators: task.validators,
      hints: assessment
        ? []
        : (task.hints ?? [
            'Read the requested final state carefully before choosing a command.',
            `The commands introduced here are ${seed.commands.join(', ')}.`,
            'Inspect the result before asking the validator to check it.',
          ]),
      suggestedSolution: task.solution,
      ...(task.alternate ? { alternateSolutions: task.alternate } : {}),
      knownIncorrectSolution: task.incorrect,
    })),
    reviewQuestions: [
      {
        type: 'multiple-choice',
        question: `Which statement best describes ${seed.title}?`,
        answers: [
          seed.mentalModel,
          'Linux accepts the task only when the exact suggested command was typed.',
          'The command works only while the guest has internet access.',
          'The operation changes files on the Windows host.',
        ],
        correctAnswer: 0,
        explanation: seed.mentalModel,
      },
    ],
  };
}

function writeJson(path: string, value: unknown): void {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, `${JSON.stringify(value, null, 2)}\n`, 'utf8');
}

function generate(): void {
  let prerequisite = 'terminal.09';
  let lessonCount = 0;

  for (const module of modules) {
    const moduleDir = join(coreRoot, module.id);
    const ids = module.lessons.map((lesson) => lesson.id);
    writeJson(join(moduleDir, 'module.json'), {
      schemaVersion: 1,
      id: module.id,
      number: module.number,
      title: module.title,
      level: module.level,
      summary: module.summary,
      outcomes: module.outcomes,
      pack: 'core',
      lessons: ids,
    });

    for (const seed of module.lessons) {
      writeJson(join(moduleDir, `${seed.id}.json`), lessonJson(module, seed, prerequisite));
      if (seed.setup) {
        const setupPath = join(setupRoot, seed.id, 'setup.sh');
        mkdirSync(dirname(setupPath), { recursive: true });
        writeFileSync(setupPath, seed.setup, 'utf8');
      }
      prerequisite = seed.id;
      lessonCount += 1;
    }
  }

  if (lessonCount !== 56) {
    throw new Error(`expected to generate 56 missing MVP lessons, generated ${lessonCount}`);
  }
  console.log(`Generated ${modules.length} modules and ${lessonCount} lessons.`);
}

generate();
