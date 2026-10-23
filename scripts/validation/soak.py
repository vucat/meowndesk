#!/usr/bin/env python3
"""External 24h soak; shares ownership/accounting with profile.py, never reads private counts."""
import sys
from profile import parser, run

if __name__ == '__main__':
    try:
        sys.exit(run(parser(soak=True).parse_args(), soak=True))
    except (ValueError, OSError) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
