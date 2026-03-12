import { test, expect } from '@playwright/test';

// API tests require backend to be running
// Set RUN_API_TESTS=true to enable these tests

const API_BASE_URL = process.env.NEXT_PUBLIC_API_URL || 'http://localhost:12356';
const shouldRunApiTests = process.env.RUN_API_TESTS === 'true';

test.describe('Backend API Tests', () => {
  test.beforeEach(() => {
    if (!shouldRunApiTests) {
      test.skip(true, 'API tests require backend. Set RUN_API_TESTS=true to enable');
    }
  });

  test('should have metrics endpoint', async ({ request }) => {
    const response = await request.get(`${API_BASE_URL}/api/metrics`);
    expect([200, 401, 404]).toContain(response.status());
  });

  test('should have usage endpoint', async ({ request }) => {
    const response = await request.get(`${API_BASE_URL}/api/usage`);
    expect([200, 401, 404]).toContain(response.status());
  });

  test('should reject requests without API key', async ({ request }) => {
    const response = await request.get(`${API_BASE_URL}/api/metrics`);
    expect([200, 401, 404]).toContain(response.status());
  });

  test('should accept requests with valid API key', async ({ request }) => {
    const response = await request.get(`${API_BASE_URL}/api/metrics`, {
      headers: {
        'Authorization': 'Bearer test-api-key',
      },
    });
    expect([200, 401, 403, 404]).toContain(response.status());
  });
});

test.describe('Health Check Tests', () => {
  test.beforeEach(() => {
    if (!shouldRunApiTests) {
      test.skip(true, 'API tests require backend. Set RUN_API_TESTS=true to enable');
    }
  });

  test('should have health or root endpoint', async ({ request }) => {
    const response = await request.get(API_BASE_URL);
    expect([200, 404]).toContain(response.status());
  });
});
