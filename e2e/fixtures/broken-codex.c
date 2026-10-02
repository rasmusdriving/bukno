/* A stand-in for a newer Codex that does not work with Bukno: it reports
   version 0.999.0 and exits as soon as it is started as an app-server.
   Built by apps/desktop/tests/codex_live.rs for the revert scenario. */
#include <stdio.h>
#include <string.h>

int main(int argc, char **argv) {
    if (argc > 1 && strcmp(argv[1], "--version") == 0) {
        printf("codex-cli 0.999.0\n");
        return 0;
    }
    fprintf(stderr, "unsupported protocol\n");
    return 3;
}
