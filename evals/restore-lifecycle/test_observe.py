import importlib
import os
import unittest
from unittest.mock import patch


class WorkingObservationTests(unittest.TestCase):
    def test_existing_work_cannot_satisfy_new_numeric_task_readiness(self):
        with patch.dict(os.environ, {'CODETERM_API_URL': 'http://127.0.0.1:7692'}):
            observe = importlib.import_module('observe')
        with patch.object(observe, 'status', return_value={'status': 'working'}), patch.object(observe, 'api') as api, patch.object(observe, 'EventWait') as wait:
            with self.assertRaises(SystemExit) as refusal:
                observe.cmd_send_long_turn(1, 'own-tab', 'opaque-task')
            self.assertEqual(refusal.exception.code, 1)
            api.assert_not_called()
            wait.assert_not_called()


if __name__ == '__main__':
    unittest.main()
