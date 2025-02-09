import cProfile
import pstats


class ProfileContext:
    """Profile context manager."""

    def __init__(self):
        self._profile = None

    def __enter__(self):
        self._profile = cProfile.Profile()
        self._profile.enable()

    def __exit__(self, exc_type, exc_val, exc_tb):
        if self._profile:
            self._profile.disable()
            results = pstats.Stats(self._profile)
            results.dump_stats("gitalong.prof")
