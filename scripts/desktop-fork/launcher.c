// Main executable of the independent Drako Codex bundle.
//
// The desktop shell checks its single-instance lock in the profile directory
// before any JavaScript runs, and without an explicit switch that directory is
// the vendor default. A second copy started from Finder therefore hands off to
// a running vendor app and exits. Finder cannot pass switches, so this launcher
// replaces itself with the real shell and names the independent profile.
#include <errno.h>
#include <libgen.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <pwd.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define REAL_EXECUTABLE "ChatGPT"
#define PROFILE_SUFFIX "/Library/Application Support/DrakoCodex/desktop-profile"
#define SWITCH "--user-data-dir="

static int make_directories(char *path) {
  for (char *cursor = path + 1; *cursor; cursor++) {
    if (*cursor != '/') continue;
    *cursor = '\0';
    if (mkdir(path, 0700) != 0 && errno != EEXIST) return -1;
    *cursor = '/';
  }
  return mkdir(path, 0700) != 0 && errno != EEXIST ? -1 : 0;
}

int main(int argc, char **argv) {
  char raw[PATH_MAX], self[PATH_MAX], directory[PATH_MAX], real[PATH_MAX];
  uint32_t size = sizeof raw;
  if (_NSGetExecutablePath(raw, &size) != 0 || realpath(raw, self) == NULL) {
    fprintf(stderr, "Drako Codex: cannot resolve the launcher path\n");
    return 70;
  }
  // dirname may modify its argument.
  strlcpy(directory, self, sizeof directory);
  if (snprintf(real, sizeof real, "%s/%s", dirname(directory), REAL_EXECUTABLE) >= (int)sizeof real) return 70;

  // Same resolution as Node's os.homedir(), which the bootstrap uses.
  const char *home = getenv("HOME");
  if (home == NULL || home[0] != '/') {
    struct passwd *entry = getpwuid(getuid());
    home = entry ? entry->pw_dir : NULL;
  }
  if (home == NULL || home[0] != '/') {
    fprintf(stderr, "Drako Codex: cannot resolve the home directory\n");
    return 70;
  }
  char profile[PATH_MAX], created[PATH_MAX], option[PATH_MAX + sizeof SWITCH];
  if (snprintf(profile, sizeof profile, "%s%s", home, PROFILE_SUFFIX) >= (int)sizeof profile) return 70;
  strlcpy(created, profile, sizeof created);
  if (make_directories(created) != 0) {
    fprintf(stderr, "Drako Codex: cannot create the profile directory: %s\n", strerror(errno));
    return 70;
  }
  snprintf(option, sizeof option, SWITCH "%s", profile);

  // A caller-supplied profile switch is dropped: this bundle has one profile.
  char **arguments = calloc((size_t)argc + 2, sizeof *arguments);
  if (arguments == NULL) return 70;
  int count = 0;
  arguments[count++] = real;
  arguments[count++] = option;
  for (int index = 1; index < argc; index++) {
    if (strncmp(argv[index], SWITCH, sizeof SWITCH - 1) == 0 || strcmp(argv[index], "--user-data-dir") == 0) continue;
    arguments[count++] = argv[index];
  }
  arguments[count] = NULL;
  execv(real, arguments);
  fprintf(stderr, "Drako Codex: cannot start %s: %s\n", real, strerror(errno));
  return 71;
}
